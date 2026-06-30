//! Event loop central de Baud.
//!
//! Sigue ADR-0005: dos hilos (GUI + PTY sincronico), sin async runtime.
//! El Term se comparte entre el hilo drain y la GUI via Arc<Mutex<Term>>.
//! El hilo drain envía UserEvent::RedrawNeeded al GUI vía EventLoopProxy.

use std::io::{ErrorKind, Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ansi::Term;
use crate::config::Config;
use crate::grid::{DEFAULT_COLS, DEFAULT_ROWS};
use crate::pty;
use crate::window::{App, UserEvent};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use winit::event_loop::EventLoop;

// ponytail: throttle a 16ms (~60fps); bajar intervalo si se quiere 120Hz.
const REDRAW_MIN_INTERVAL: Duration = Duration::from_millis(16);

const METRICS_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// Comandos del GUI al hilo PTY.
pub enum PtyCommand {
    /// Bytes de input para escribir al master.
    Input(Vec<u8>),
    /// Resize: el child debe actualizar su winsize.
    Resize { rows: u16, cols: u16 },
    /// Shutdown: enviar SIGHUP al child y esperar.
    Shutdown,
}

/// Eventos que el hilo PTY envía al hilo drain.
pub enum PtyEvent {
    /// Datos crudos leidos del master PTY.
    Output(Vec<u8>),
    /// El child termino (EOF en master fd). -1 si no se conoce el exit code.
    Exited(i32),
    /// Error de I/O del PTY (lectura devuelve error, broken pipe, etc.).
    IoError(String),
}

/// Retorna true si paso el intervalo minimo desde el ultimo redraw.
pub(crate) fn should_redraw(last: Instant, now: Instant) -> bool {
    now.duration_since(last) >= REDRAW_MIN_INTERVAL
}

struct DrainMetrics {
    redraws: u64,
    bytes: u64,
    period_start: Instant,
}

impl DrainMetrics {
    fn new() -> Self {
        Self {
            redraws: 0,
            bytes: 0,
            period_start: Instant::now(),
        }
    }

    fn record_bytes(&mut self, n: usize) {
        self.bytes += n as u64;
    }

    fn record_redraw(&mut self) {
        self.redraws += 1;
    }

    fn maybe_log(&mut self) {
        let elapsed = self.period_start.elapsed();
        if elapsed < METRICS_LOG_INTERVAL {
            return;
        }
        let secs = elapsed.as_secs_f64();
        tracing::debug!(
            target: "baud::pipeline",
            "drain: {:.0} redraws/s, {:.0} bytes/s",
            self.redraws as f64 / secs,
            self.bytes as f64 / secs,
        );
        *self = Self::new();
    }
}

fn send_redraw(proxy_slot: &Arc<Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>>) {
    if let Some(proxy) = proxy_slot
        .lock()
        .expect("proxy mutex poisoned en drain")
        .as_ref()
    {
        let _ = proxy.send_event(UserEvent::RedrawNeeded);
    }
}

fn send_title_and_clipboard(
    proxy_slot: &Arc<Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>>,
    title: Option<String>,
    clipboard_pending: Option<(u8, bool)>,
) {
    if let Some(proxy) = proxy_slot
        .lock()
        .expect("proxy mutex poisoned en drain")
        .as_ref()
    {
        if let Some(t) = title {
            let _ = proxy.send_event(UserEvent::SetTitle(t));
        }
        if let Some((target, bell)) = clipboard_pending {
            let _ = proxy.send_event(UserEvent::ReadClipboard(target, bell));
        }
    }
}

fn handle_non_output_pty_event(
    event: PtyEvent,
    proxy_slot: &Arc<Mutex<Option<winit::event_loop::EventLoopProxy<UserEvent>>>>,
) {
    match event {
        PtyEvent::Exited(code) => {
            tracing::info!("child termino con codigo {code}");
            if let Some(proxy) = proxy_slot
                .lock()
                .expect("proxy mutex poisoned en drain")
                .as_ref()
            {
                let _ = proxy.send_event(UserEvent::PtyExited(code));
            }
        }
        PtyEvent::IoError(msg) => {
            tracing::warn!("error de I/O del PTY: {msg}");
            if let Some(proxy) = proxy_slot
                .lock()
                .expect("proxy mutex poisoned en drain")
                .as_ref()
            {
                let _ = proxy.send_event(UserEvent::PtyError(msg));
            }
        }
        PtyEvent::Output(_) => unreachable!("Output se maneja en el match principal"),
    }
}

/// Punto de entrada del event loop.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app_config = Config::load();
    let process_cfg = app_config.process_config();
    let startup_command = process_cfg.startup_command.clone();

    let master = pty::spawn_with(&process_cfg)?;
    master.set_winsize(DEFAULT_ROWS as u16, DEFAULT_COLS as u16)?;

    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<PtyCommand>();
    let tx_response = tx_gui_to_pty.clone();

    let term = Arc::new(Mutex::new(Term::new()));

    let term_drain = Arc::clone(&term);
    let proxy_for_drain = Arc::new(Mutex::new(
        None::<winit::event_loop::EventLoopProxy<UserEvent>>,
    ));
    let proxy_for_drain_clone = Arc::clone(&proxy_for_drain);

    let drain_handle = thread::spawn(move || {
        let mut parser = vte::Parser::new();
        let mut metrics = DrainMetrics::new();
        let mut last_redraw = Instant::now();
        let mut pending_redraw = false;

        loop {
            if pending_redraw && should_redraw(last_redraw, Instant::now()) {
                send_redraw(&proxy_for_drain_clone);
                metrics.record_redraw();
                last_redraw = Instant::now();
                pending_redraw = false;
            }

            let timeout = if pending_redraw {
                REDRAW_MIN_INTERVAL.saturating_sub(last_redraw.elapsed())
            } else {
                Duration::from_secs(3600)
            };

            let event = match rx_pty_to_gui.recv_timeout(timeout) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    metrics.maybe_log();
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };

            match event {
                PtyEvent::Output(first) => {
                    let mut chunks = vec![first];
                    let mut deferred: Vec<PtyEvent> = Vec::new();
                    while let Ok(more) = rx_pty_to_gui.try_recv() {
                        match more {
                            PtyEvent::Output(bytes) => chunks.push(bytes),
                            other => deferred.push(other),
                        }
                    }

                    let (response, title, clipboard_pending, total_bytes) = {
                        let mut term_guard =
                            term_drain.lock().expect("term mutex poisoned en drain");
                        let mut total_bytes = 0usize;
                        for bytes in &chunks {
                            parser.advance(&mut *term_guard, bytes);
                            total_bytes += bytes.len();
                        }
                        term_guard.mark_dirty();
                        let response = term_guard.take_pty_response();
                        let title = term_guard.take_title_if_dirty();
                        let clipboard_pending = term_guard.take_clipboard_read_pending();
                        (response, title, clipboard_pending, total_bytes)
                    };
                    metrics.record_bytes(total_bytes);

                    if !response.is_empty() {
                        if let Err(e) = tx_response.send(PtyCommand::Input(response)) {
                            tracing::warn!(
                                "drain: no se pudo reenviar respuesta PTY ({e}); query descartada"
                            );
                        }
                    }
                    send_title_and_clipboard(&proxy_for_drain_clone, title, clipboard_pending);

                    if should_redraw(last_redraw, Instant::now()) {
                        send_redraw(&proxy_for_drain_clone);
                        metrics.record_redraw();
                        last_redraw = Instant::now();
                        pending_redraw = false;
                    } else {
                        pending_redraw = true;
                    }

                    for deferred_event in deferred {
                        handle_non_output_pty_event(deferred_event, &proxy_for_drain_clone);
                    }
                }
                other => handle_non_output_pty_event(other, &proxy_for_drain_clone),
            }

            metrics.maybe_log();
        }
    });

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    *proxy_for_drain
        .lock()
        .expect("proxy mutex poisoned al setear") = Some(proxy);

    let pty_tx = Arc::new(Mutex::new(Some(tx_gui_to_pty)));

    let mut app = App::new(Arc::clone(&term), Arc::clone(&pty_tx), app_config);

    let pty_thread = thread::spawn(move || {
        let mut master = master;
        let mut buf = [0u8; 4096];

        if let Ok(flags) = fcntl(master.fd(), FcntlArg::F_GETFL) {
            let nonblock = OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK;
            let _ = fcntl(master.fd(), FcntlArg::F_SETFL(nonblock));
        }

        loop {
            let mut had_activity = false;

            loop {
                match master.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx_pty_to_gui.send(PtyEvent::Exited(-1));
                        return;
                    }
                    Ok(n) => {
                        had_activity = true;
                        if tx_pty_to_gui
                            .send(PtyEvent::Output(buf[..n].to_vec()))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) => {
                        tracing::warn!("error de I/O en PTY: {e}");
                        let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                        return;
                    }
                }
            }

            while let Ok(cmd) = rx_gui_to_pty.try_recv() {
                had_activity = true;
                match cmd {
                    PtyCommand::Input(bytes) => {
                        let _ = master.write_all(&bytes);
                    }
                    PtyCommand::Resize { rows, cols } => {
                        if let Err(e) = master.set_winsize(rows, cols) {
                            tracing::warn!("error al setear winsize: {e}");
                        }
                    }
                    PtyCommand::Shutdown => {
                        let sent = master.send_sighup();
                        if sent {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        return;
                    }
                }
            }

            if !had_activity {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });

    if let Some(cmd) = startup_command {
        if let Some(tx) = pty_tx.lock().expect("pty_tx mutex poisoned").as_ref() {
            let _ = tx.send(PtyCommand::Input(format!("{cmd}\n").into_bytes()));
        }
    }

    tracing::info!("event loop iniciado, shell corriendo en PTY");

    event_loop.run_app(&mut app)?;

    drop(pty_tx);
    drop(term);
    let _ = pty_thread.join();
    let _ = drain_handle.join();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_redraw_respeta_16ms() {
        let t0 = Instant::now();
        assert!(!should_redraw(t0, t0 + Duration::from_millis(5)));
        assert!(should_redraw(t0, t0 + Duration::from_millis(20)));
    }

    #[test]
    fn test_set_winsize_after_spawn() {
        let master = pty::spawn("bash", &["-c", "exit"]).expect("spawn fallo");
        assert!(master.set_winsize(24, 80).is_ok());
    }

    #[test]
    fn test_pty_eof_no_panic() {
        let mut master = pty::spawn("bash", &["-c", "exit"]).expect("spawn fallo");
        let mut buf = [0u8; 4096];
        loop {
            match master.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
}
