//! Telemetría del hilo GUI: heartbeat del event loop y fase activa del handler.
//!
//! Coste casi-cero en hot path (`ping` / `enter` / `leave`):
//! - Solo atomics + seqlock (sin `Mutex`, sin `mpsc` por heartbeat).
//! - El hilo watchdog solo *lee* cada 2s; el GUI nunca espera.
//! - `HandlerGuard` clona el `Arc` (refcount) para no prestar `&App`.

use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL_MS: u64 = 2_000;
const SLOW_HANDLER_WARN_MS: u64 = 250;

/// Instantáneo de telemetría legible en logs y tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySnapshot {
    pub heartbeat_age: Duration,
    pub current_handler: Option<&'static str>,
    pub current_handler_age: Option<Duration>,
    pub term_lock_busy: u64,
    pub slow_handlers: u64,
    pub stalls: u64,
}

/// Estado compartido GUI ↔ hilo `baud-watchdog` (lock-free en hot path).
///
/// Escritura solo desde el hilo GUI; el watchdog solo lee. Un seqlock evita
/// que `snapshot` combine `ptr`/`len` de dos `enter` distintos.
#[derive(Debug)]
struct LoopTelemetry {
    epoch: Instant,
    last_heartbeat_ms: AtomicU64,
    /// Seqlock: impar = escritura en curso; par = estable.
    handler_seq: AtomicU64,
    handler_ptr: AtomicPtr<u8>,
    handler_len: AtomicUsize,
    handler_started_ms: AtomicU64,
    term_lock_busy: AtomicU64,
    slow_handlers: AtomicU64,
    stalls: AtomicU64,
}

impl LoopTelemetry {
    fn new() -> Self {
        let epoch = Instant::now();
        Self {
            epoch,
            last_heartbeat_ms: AtomicU64::new(0),
            handler_seq: AtomicU64::new(0),
            handler_ptr: AtomicPtr::new(std::ptr::null_mut()),
            handler_len: AtomicUsize::new(0),
            handler_started_ms: AtomicU64::new(0),
            term_lock_busy: AtomicU64::new(0),
            slow_handlers: AtomicU64::new(0),
            stalls: AtomicU64::new(0),
        }
    }

    #[inline]
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    #[inline]
    fn ping(&self) {
        self.last_heartbeat_ms
            .store(self.now_ms(), Ordering::Relaxed);
    }

    #[inline]
    fn enter(&self, name: &'static str) {
        let now = self.now_ms();
        self.handler_seq.fetch_add(1, Ordering::Relaxed); // → impar
        self.handler_started_ms.store(now, Ordering::Relaxed);
        self.handler_len.store(name.len(), Ordering::Relaxed);
        self.handler_ptr
            .store(name.as_ptr().cast_mut(), Ordering::Relaxed);
        self.handler_seq.fetch_add(1, Ordering::Release); // → par
    }

    #[inline]
    fn leave(&self, name: &'static str) {
        self.handler_seq.fetch_add(1, Ordering::Relaxed); // → impar
        let ptr = self.handler_ptr.load(Ordering::Relaxed);
        let matches = !ptr.is_null() && std::ptr::eq(ptr, name.as_ptr());
        let started = self.handler_started_ms.load(Ordering::Relaxed);
        if matches {
            self.handler_ptr
                .store(std::ptr::null_mut(), Ordering::Relaxed);
            self.handler_len.store(0, Ordering::Relaxed);
        }
        self.handler_seq.fetch_add(1, Ordering::Release); // → par

        if !matches {
            return;
        }
        let elapsed_ms = self.now_ms().saturating_sub(started);
        if elapsed_ms < SLOW_HANDLER_WARN_MS {
            return;
        }
        self.slow_handlers.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(
            target: "baud::watchdog",
            handler = name,
            elapsed_ms,
            "handler lento del event loop"
        );
    }

    #[inline]
    fn note_term_lock_busy(&self) {
        self.term_lock_busy.fetch_add(1, Ordering::Relaxed);
    }

    fn current_handler_and_start(&self) -> (Option<&'static str>, u64) {
        loop {
            let s1 = self.handler_seq.load(Ordering::Acquire);
            if s1 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let ptr = self.handler_ptr.load(Ordering::Relaxed);
            let len = self.handler_len.load(Ordering::Relaxed);
            let started = self.handler_started_ms.load(Ordering::Relaxed);
            let s2 = self.handler_seq.load(Ordering::Acquire);
            if s1 != s2 {
                continue;
            }
            if ptr.is_null() {
                return (None, 0);
            }
            // SAFETY: ptr+len vienen del mismo `enter` (seqlock estable) sobre
            // un literal `&'static str`.
            let name =
                unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
            return (Some(name), started);
        }
    }

    fn snapshot(&self) -> TelemetrySnapshot {
        let now = self.now_ms();
        let heartbeat_age = Duration::from_millis(
            now.saturating_sub(self.last_heartbeat_ms.load(Ordering::Relaxed)),
        );
        let (current_handler, started) = self.current_handler_and_start();
        let current_handler_age =
            current_handler.map(|_| Duration::from_millis(now.saturating_sub(started)));
        TelemetrySnapshot {
            heartbeat_age,
            current_handler,
            current_handler_age,
            term_lock_busy: self.term_lock_busy.load(Ordering::Relaxed),
            slow_handlers: self.slow_handlers.load(Ordering::Relaxed),
            stalls: self.stalls.load(Ordering::Relaxed),
        }
    }

    fn uptime_secs(&self) -> u64 {
        self.epoch.elapsed().as_secs()
    }
}

/// RAII: marca la fase activa; al dropear mide duración (warn si es lenta).
///
/// Clona el `Arc` (refcount atómico) para no bloquear `&mut App` mientras
/// el guard vive. El coste relevante del hot path son los atomics, no el mutex.
pub struct HandlerGuard {
    telemetry: Arc<LoopTelemetry>,
    name: &'static str,
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        self.telemetry.leave(self.name);
    }
}

/// Vigilancia + telemetría del event loop GUI.
#[derive(Clone)]
pub struct EventLoopWatchdog {
    telemetry: Arc<LoopTelemetry>,
}

impl EventLoopWatchdog {
    /// Lanza el hilo de vigilancia. Debe llamarse una vez al arrancar la app.
    pub fn spawn() -> Self {
        let telemetry = Arc::new(LoopTelemetry::new());
        telemetry.ping();
        let tel = Arc::clone(&telemetry);
        thread::Builder::new()
            .name("baud-watchdog".into())
            .spawn(move || loop {
                thread::sleep(HEARTBEAT_INTERVAL);
                let snap = tel.snapshot();
                if snap.heartbeat_age.as_millis() < u128::from(HEARTBEAT_INTERVAL_MS) {
                    continue;
                }
                let stalls = tel.stalls.fetch_add(1, Ordering::Relaxed) + 1;
                let handler = snap.current_handler.unwrap_or("idle");
                let handler_ms = snap
                    .current_handler_age
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                tracing::warn!(
                    target: "baud::watchdog",
                    heartbeat_age_ms = snap.heartbeat_age.as_millis() as u64,
                    handler,
                    handler_age_ms = handler_ms,
                    term_lock_busy = snap.term_lock_busy,
                    slow_handlers = snap.slow_handlers,
                    stalls,
                    uptime_s = tel.uptime_secs(),
                    "event loop sin heartbeat — posible bloqueo (GPU, mutex, I/O)"
                );
            })
            .expect("no se pudo iniciar watchdog");
        Self { telemetry }
    }

    /// Llamar desde `about_to_wait` en cada iteracion del event loop.
    #[inline]
    pub fn ping(&self) {
        self.telemetry.ping();
    }

    /// Marca el handler activo.
    #[inline]
    pub fn enter(&self, name: &'static str) -> HandlerGuard {
        self.telemetry.enter(name);
        HandlerGuard {
            telemetry: Arc::clone(&self.telemetry),
            name,
        }
    }

    /// Contador de veces que el hot path del mouse no pudo tomar el Term.
    #[inline]
    pub fn note_term_lock_busy(&self) {
        self.telemetry.note_term_lock_busy();
    }

    /// Instantáneo para tests / diagnóstico (camino frío).
    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.telemetry.snapshot()
    }

    /// Instancia sin hilo de vigilancia (tests).
    pub fn noop() -> Self {
        let telemetry = Arc::new(LoopTelemetry::new());
        telemetry.ping();
        Self { telemetry }
    }
}

/// Nombre estable de fase para un `WindowEvent` (telemetría).
pub fn window_event_phase(event: &winit::event::WindowEvent) -> &'static str {
    use winit::event::WindowEvent;
    match event {
        WindowEvent::RedrawRequested => "RedrawRequested",
        WindowEvent::Resized(_) => "Resized",
        WindowEvent::CursorMoved { .. } => "CursorMoved",
        WindowEvent::CursorEntered { .. } => "CursorEntered",
        WindowEvent::CursorLeft { .. } => "CursorLeft",
        WindowEvent::MouseInput { .. } => "MouseInput",
        WindowEvent::MouseWheel { .. } => "MouseWheel",
        WindowEvent::KeyboardInput { .. } => "KeyboardInput",
        WindowEvent::ModifiersChanged(_) => "ModifiersChanged",
        WindowEvent::Ime(_) => "Ime",
        WindowEvent::Focused(_) => "Focused",
        WindowEvent::CloseRequested => "CloseRequested",
        WindowEvent::ScaleFactorChanged { .. } => "ScaleFactorChanged",
        _ => "WindowEvent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_leave_tracks_current_handler() {
        let wd = EventLoopWatchdog::noop();
        {
            let _g = wd.enter("RedrawRequested");
            let snap = wd.snapshot();
            assert_eq!(snap.current_handler, Some("RedrawRequested"));
            assert!(snap.current_handler_age.is_some());
        }
        assert_eq!(wd.snapshot().current_handler, None);
    }

    #[test]
    fn ping_resets_heartbeat_age() {
        let wd = EventLoopWatchdog::noop();
        thread::sleep(Duration::from_millis(5));
        wd.ping();
        assert!(wd.snapshot().heartbeat_age < Duration::from_millis(50));
    }

    #[test]
    fn note_term_lock_busy_increments() {
        let wd = EventLoopWatchdog::noop();
        wd.note_term_lock_busy();
        wd.note_term_lock_busy();
        assert_eq!(wd.snapshot().term_lock_busy, 2);
    }

    #[test]
    fn leave_mismatched_name_does_not_clear_other_handler() {
        let wd = EventLoopWatchdog::noop();
        let _g = wd.enter("RedrawRequested");
        wd.telemetry.leave("CursorMoved");
        assert_eq!(wd.snapshot().current_handler, Some("RedrawRequested"));
    }
}
