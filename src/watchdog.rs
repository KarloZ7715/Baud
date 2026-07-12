//! Telemetría del hilo GUI: heartbeat del event loop y fase activa del handler.
//!
//! Coste casi-cero en hot path (`ping` / `enter` / `leave`):
//! - `ping`: un `fetch_add` de generación (sin Instant).
//! - `enter`/`leave`: atomics ordenados + `*const` (sin Mutex/mpsc/Arc::clone).
//! - El hilo watchdog lee cada 2s y materializa timestamps al detectar stall.

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
/// Escritura solo desde el hilo GUI. Orden: `started`/`len` antes de publicar
/// `ptr` con `Release`; lectores hacen `Acquire` en `ptr` y luego leen el resto.
#[derive(Debug)]
struct LoopTelemetry {
    epoch: Instant,
    /// Generación de heartbeat (incrementa en cada `ping`).
    heartbeat_gen: AtomicU64,
    /// Ms desde `epoch` en el último `ping` (rellenado perezoso en snapshot/stall).
    last_heartbeat_ms: AtomicU64,
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
            heartbeat_gen: AtomicU64::new(0),
            last_heartbeat_ms: AtomicU64::new(0),
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

    /// Hot path: un solo atomic. El timestamp lo materializa el watchdog.
    #[inline]
    fn ping(&self) {
        self.heartbeat_gen.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn enter(&self, name: &'static str) {
        let now = self.now_ms();
        self.handler_started_ms.store(now, Ordering::Relaxed);
        self.handler_len.store(name.len(), Ordering::Relaxed);
        self.handler_ptr
            .store(name.as_ptr().cast_mut(), Ordering::Release);
    }

    #[inline]
    fn leave(&self, name: &'static str) {
        let ptr = self.handler_ptr.load(Ordering::Relaxed);
        let matches = !ptr.is_null() && std::ptr::eq(ptr, name.as_ptr());
        if !matches {
            return;
        }
        let started = self.handler_started_ms.load(Ordering::Relaxed);
        self.handler_ptr
            .store(std::ptr::null_mut(), Ordering::Release);
        self.handler_len.store(0, Ordering::Relaxed);

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
        let ptr = self.handler_ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            return (None, 0);
        }
        let len = self.handler_len.load(Ordering::Relaxed);
        let started = self.handler_started_ms.load(Ordering::Relaxed);
        // SAFETY: `enter` escribe len/started antes del Release de ptr; somos
        // Acquire aquí. Solo literales `&'static str`.
        let name = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
        (Some(name), started)
    }

    /// Snapshot usado en WARN de stall (edad ya conocida por el hilo watchdog).
    fn snapshot_for_stall(&self, heartbeat_age: Duration) -> TelemetrySnapshot {
        let now = self.now_ms();
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

    /// Diagnóstico/tests: edad desde el último materialize del watchdog o noop.
    fn snapshot_diagnostic(&self) -> TelemetrySnapshot {
        let now = self.now_ms();
        let last = self.last_heartbeat_ms.load(Ordering::Relaxed);
        let heartbeat_age = Duration::from_millis(now.saturating_sub(last));
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
/// Guarda `*const LoopTelemetry` (el `Arc` del watchdog mantiene vivo el
/// allocation). El guard no se envía entre hilos.
pub struct HandlerGuard {
    telemetry: *const LoopTelemetry,
    name: &'static str,
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        // SAFETY: `enter` solo se llama mientras el Arc del watchdog vive;
        // el guard no escapa del stack del handler.
        unsafe {
            (*self.telemetry).leave(self.name);
        }
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
        Self::spawn_if(true)
    }

    /// Lanza el hilo de vigilancia solo si `enabled` es `true`.
    /// Si es `false`, devuelve una instancia noop sin hilo ni logs.
    pub fn spawn_if(enabled: bool) -> Self {
        if !enabled {
            return Self::noop();
        }
        let telemetry = Arc::new(LoopTelemetry::new());
        telemetry.ping();
        let tel = Arc::clone(&telemetry);
        thread::Builder::new()
            .name("baud-watchdog".into())
            .spawn(move || {
                let mut last_gen = tel.heartbeat_gen.load(Ordering::Relaxed);
                let mut last_seen_at = Instant::now();
                loop {
                    thread::sleep(HEARTBEAT_INTERVAL);
                    let gen = tel.heartbeat_gen.load(Ordering::Relaxed);
                    if gen != last_gen {
                        last_gen = gen;
                        last_seen_at = Instant::now();
                        tel.last_heartbeat_ms.store(tel.now_ms(), Ordering::Relaxed);
                        continue;
                    }
                    let age = last_seen_at.elapsed();
                    if age.as_millis() < u128::from(HEARTBEAT_INTERVAL_MS) {
                        continue;
                    }
                    let stalls = tel.stalls.fetch_add(1, Ordering::Relaxed) + 1;
                    let snap = tel.snapshot_for_stall(age);
                    let handler = snap.current_handler.unwrap_or("idle");
                    let handler_ms = snap
                        .current_handler_age
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    tracing::warn!(
                        target: "baud::watchdog",
                        heartbeat_age_ms = age.as_millis() as u64,
                        handler,
                        handler_age_ms = handler_ms,
                        term_lock_busy = snap.term_lock_busy,
                        slow_handlers = snap.slow_handlers,
                        stalls,
                        uptime_s = tel.uptime_secs(),
                        "event loop sin heartbeat — posible bloqueo (GPU, mutex, I/O)"
                    );
                }
            })
            .expect("could not start watchdog");
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
            telemetry: Arc::as_ptr(&self.telemetry),
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
        self.telemetry.snapshot_diagnostic()
    }

    /// Instancia sin hilo de vigilancia (tests).
    pub fn noop() -> Self {
        let telemetry = Arc::new(LoopTelemetry::new());
        telemetry.ping();
        telemetry
            .last_heartbeat_ms
            .store(telemetry.now_ms(), Ordering::Relaxed);
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
    fn ping_advances_generation_for_stall_detection() {
        let wd = EventLoopWatchdog::noop();
        let before = wd.telemetry.heartbeat_gen.load(Ordering::Relaxed);
        wd.ping();
        let after = wd.telemetry.heartbeat_gen.load(Ordering::Relaxed);
        assert!(after > before);
    }

    #[test]
    fn ping_resets_heartbeat_age() {
        let wd = EventLoopWatchdog::noop();
        // Simula materialize del watchdog tras un ping.
        wd.telemetry
            .last_heartbeat_ms
            .store(wd.telemetry.now_ms(), Ordering::Relaxed);
        thread::sleep(Duration::from_millis(5));
        wd.ping();
        wd.telemetry
            .last_heartbeat_ms
            .store(wd.telemetry.now_ms(), Ordering::Relaxed);
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
