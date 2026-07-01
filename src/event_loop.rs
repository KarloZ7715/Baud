//! Event loop central de Baud.
//!
//! Sigue ADR-0005: dos hilos (GUI + PTY sincronico), sin async runtime.
//! El Term se comparte entre el hilo drain y la GUI via Arc<Mutex<Term>>.
//! El hilo drain envía UserEvent::RedrawNeeded al GUI vía EventLoopProxy.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsFd;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ansi::Term;
use crate::config::Config;
use crate::grid::{DEFAULT_COLS, DEFAULT_ROWS};
use crate::pty;
use crate::window::{App, UserEvent};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::eventfd::{EfdFlags, EventFd};
use winit::event_loop::EventLoop;

// ponytail: throttle a 16ms (~60fps); bajar intervalo si se quiere 120Hz.
const REDRAW_MIN_INTERVAL: Duration = Duration::from_millis(16);

const METRICS_LOG_INTERVAL: Duration = Duration::from_secs(5);

// ponytail: tope de bytes por pasada del drain; suelta el mutex del Term para la GUI.
const DRAIN_MAX_BYTES_PER_PASS: usize = 256 * 1024;

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

/// Envia comandos al hilo PTY y despierta `poll` via eventfd.
#[derive(Clone)]
pub struct PtyCommandSender {
    tx: mpsc::Sender<PtyCommand>,
    wakeup: Arc<EventFd>,
}

impl PtyCommandSender {
    pub fn send(&self, cmd: PtyCommand) -> Result<(), mpsc::SendError<PtyCommand>> {
        self.tx.send(cmd)?;
        let _ = self.wakeup.write(1);
        Ok(())
    }
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

/// Agrupa `Output` del canal hasta `max_bytes`; el sobrante va en el vector devuelto.
fn coalesce_output_chunks(
    first: Vec<u8>,
    rx: &mpsc::Receiver<PtyEvent>,
    max_bytes: usize,
) -> (Vec<Vec<u8>>, Vec<PtyEvent>) {
    let mut chunks = vec![first];
    let mut total = chunks[0].len();
    let mut deferred: Vec<PtyEvent> = Vec::new();

    while let Ok(more) = rx.try_recv() {
        match more {
            PtyEvent::Output(bytes) => {
                if total + bytes.len() > max_bytes {
                    deferred.push(PtyEvent::Output(bytes));
                    while let Ok(rest) = rx.try_recv() {
                        match rest {
                            PtyEvent::Output(b) => deferred.push(PtyEvent::Output(b)),
                            other => deferred.push(other),
                        }
                    }
                    break;
                }
                total += bytes.len();
                chunks.push(bytes);
            }
            other => deferred.push(other),
        }
    }

    (chunks, deferred)
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

fn drain_eventfd(efd: &EventFd) {
    loop {
        match efd.read() {
            Ok(_) => continue,
            Err(nix::errno::Errno::EAGAIN) => break,
            Err(_) => break,
        }
    }
}

fn process_pty_commands(master: &mut pty::Pty, rx_gui_to_pty: &mpsc::Receiver<PtyCommand>) -> bool {
    let mut shutdown = false;
    while let Ok(cmd) = rx_gui_to_pty.try_recv() {
        match cmd {
            PtyCommand::Input(bytes) => {
                tracing::trace!("pty_thread: write {} bytes: {:02x?}", bytes.len(), bytes);
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
                shutdown = true;
                break;
            }
        }
    }
    shutdown
}

fn master_poll_ready(revents: PollFlags) -> bool {
    revents.intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR)
}

enum ReadMasterOutcome {
    Done,
    Eof,
    DrainClosed,
    IoError,
}

fn read_master_available(
    master: &mut pty::Pty,
    buf: &mut [u8],
    tx_pty_to_gui: &mpsc::Sender<PtyEvent>,
) -> ReadMasterOutcome {
    loop {
        match master.read(buf) {
            Ok(0) => return ReadMasterOutcome::Eof,
            Ok(n) => {
                tracing::trace!("pty_thread: read {} bytes: {:02x?}", n, &buf[..n.min(40)]);
                if tx_pty_to_gui
                    .send(PtyEvent::Output(buf[..n].to_vec()))
                    .is_err()
                {
                    return ReadMasterOutcome::DrainClosed;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return ReadMasterOutcome::Done,
            Err(e) => {
                tracing::warn!("error de I/O en PTY: {e}");
                let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                return ReadMasterOutcome::IoError;
            }
        }
    }
}

/// Lanza el hilo timer de parpadeo.
///
/// Cada `blink_interval/2` consulta `Term::has_blink_stuff`; si hay cursor o
/// celdas SGR 5 que parpadean, marca el term dirty y envia `RedrawNeeded` por
/// el proxy. cuando el parpadeo esta desactivado (`blink_interval_ms == 0` o
/// nada que parpadear), el hilo duerme sin enviar eventos.
// ponytail: hilo detached; muere al salir del proceso. stop flag si se quiere
// shutdown explicito, sobra para una app interactiva.
fn spawn_blink_timer(term: Arc<Mutex<Term>>, proxy: winit::event_loop::EventLoopProxy<UserEvent>) {
    thread::spawn(move || loop {
        let interval_ms = match term.lock() {
            Ok(g) => g.blink_interval_ms,
            Err(e) => {
                tracing::warn!("blink timer: term mutex envenenado, deteniendo: {e}");
                return;
            }
        };
        if interval_ms == 0 {
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        let interval = Duration::from_millis(interval_ms);
        thread::sleep(interval / 2);
        let blinking = match term.lock() {
            Ok(mut g) => {
                if g.has_blink_stuff() {
                    g.mark_dirty();
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                tracing::warn!("blink timer: term mutex envenenado, deteniendo: {e}");
                return;
            }
        };
        if blinking {
            let _ = proxy.send_event(UserEvent::RedrawNeeded);
        }
    });
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
///
/// Crea el PTY, lanza el shell configurado, y arranca los hilos necesarios.
/// Retorna cuando se cierra la ventana (event_loop.exit()).
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app_config = Config::load();
    let process_cfg = app_config.process_config();
    let startup_command = process_cfg.startup_command.clone();

    let master = pty::spawn_with(&process_cfg)?;
    master.set_winsize(DEFAULT_ROWS as u16, DEFAULT_COLS as u16)?;

    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<PtyCommand>();
    let wakeup =
        Arc::new(EventFd::from_flags(EfdFlags::EFD_NONBLOCK).expect("no se pudo crear eventfd"));
    let cmd_sender = PtyCommandSender {
        tx: tx_gui_to_pty,
        wakeup: Arc::clone(&wakeup),
    };
    let tx_response = cmd_sender.clone();

    let term = Arc::new(Mutex::new({
        let mut t = Term::new_with_scrollback(app_config.scrollback_max_lines());
        app_config.apply_to_term(&mut t);
        t
    }));

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
        let mut output_backlog: VecDeque<Vec<u8>> = VecDeque::new();

        loop {
            if pending_redraw && should_redraw(last_redraw, Instant::now()) {
                send_redraw(&proxy_for_drain_clone);
                metrics.record_redraw();
                last_redraw = Instant::now();
                pending_redraw = false;
            }

            let timeout = if pending_redraw {
                REDRAW_MIN_INTERVAL.saturating_sub(last_redraw.elapsed())
            } else if output_backlog.is_empty() {
                Duration::from_secs(3600)
            } else {
                Duration::ZERO
            };

            let first = if let Some(chunk) = output_backlog.pop_front() {
                chunk
            } else {
                match rx_pty_to_gui.recv_timeout(timeout) {
                    Ok(PtyEvent::Output(bytes)) => bytes,
                    Ok(other) => {
                        handle_non_output_pty_event(other, &proxy_for_drain_clone);
                        metrics.maybe_log();
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        metrics.maybe_log();
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };

            let (chunks, deferred) =
                coalesce_output_chunks(first, &rx_pty_to_gui, DRAIN_MAX_BYTES_PER_PASS);

            let (response, title, clipboard_pending, total_bytes) = {
                let mut term_guard = term_drain.lock().expect("term mutex poisoned en drain");
                let mut total_bytes = 0usize;
                for bytes in &chunks {
                    parser.advance(&mut *term_guard, bytes);
                    total_bytes += bytes.len();
                }
                term_guard.mark_dirty();
                term_guard.reset_blink_phase();
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

            for event in deferred {
                match event {
                    PtyEvent::Output(bytes) => output_backlog.push_back(bytes),
                    other => handle_non_output_pty_event(other, &proxy_for_drain_clone),
                }
            }

            metrics.maybe_log();
        }
    });

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    *proxy_for_drain
        .lock()
        .expect("proxy mutex poisoned al setear") = Some(proxy.clone());

    spawn_blink_timer(Arc::clone(&term), proxy);

    let pty_tx = Arc::new(Mutex::new(Some(cmd_sender)));

    let mut app = App::new(Arc::clone(&term), Arc::clone(&pty_tx), app_config);

    let wakeup_pty = Arc::clone(&wakeup);
    let pty_thread = thread::spawn(move || {
        let mut master = master;
        let mut buf = [0u8; 4096];

        // ponytail: master no-bloqueante; tras poll legible, leer en rafaga hasta WouldBlock.
        if let Ok(flags) = fcntl(master.fd(), FcntlArg::F_GETFL) {
            let nonblock = OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK;
            let _ = fcntl(master.fd(), FcntlArg::F_SETFL(nonblock));
        }

        loop {
            let (master_ready, wakeup_ready) = {
                let mut poll_fds = [
                    PollFd::new(master.fd().as_fd(), PollFlags::POLLIN),
                    PollFd::new(wakeup_pty.as_fd(), PollFlags::POLLIN),
                ];
                loop {
                    match poll(&mut poll_fds, PollTimeout::NONE) {
                        Ok(_) => break,
                        Err(nix::errno::Errno::EINTR) => continue,
                        Err(e) => {
                            tracing::warn!("poll en hilo PTY fallo: {e}");
                            let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                            return;
                        }
                    }
                }
                let master_ready = poll_fds[0].revents().is_some_and(master_poll_ready);
                let wakeup_ready = poll_fds[1]
                    .revents()
                    .is_some_and(|r| r.contains(PollFlags::POLLIN));
                (master_ready, wakeup_ready)
            };

            if wakeup_ready {
                drain_eventfd(&wakeup_pty);
                if process_pty_commands(&mut master, &rx_gui_to_pty) {
                    return;
                }
            }

            if master_ready {
                match read_master_available(&mut master, &mut buf, &tx_pty_to_gui) {
                    ReadMasterOutcome::Done => {}
                    ReadMasterOutcome::Eof => {
                        let _ = tx_pty_to_gui.send(PtyEvent::Exited(-1));
                        return;
                    }
                    ReadMasterOutcome::DrainClosed | ReadMasterOutcome::IoError => return,
                }
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
    use std::os::fd::AsFd;

    use nix::poll::PollTimeout;
    use nix::poll::{poll, PollFd, PollFlags};
    use nix::sys::eventfd::{EfdFlags, EventFd};

    #[test]
    fn test_coalesce_respeta_tope_bytes() {
        let (tx, rx) = mpsc::channel::<PtyEvent>();
        tx.send(PtyEvent::Output(vec![0u8; 100])).unwrap();
        tx.send(PtyEvent::Output(vec![0u8; 100])).unwrap();

        let (chunks, deferred) = coalesce_output_chunks(vec![0u8; 150], &rx, 200);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 150);
        assert_eq!(deferred.len(), 2);
    }

    #[test]
    fn test_should_redraw_respeta_16ms() {
        let t0 = Instant::now();
        assert!(!should_redraw(t0, t0 + Duration::from_millis(5)));
        assert!(should_redraw(t0, t0 + Duration::from_millis(20)));
    }

    #[test]
    fn test_eventfd_despierta_poll() {
        let efd = EventFd::from_flags(EfdFlags::EFD_NONBLOCK).expect("eventfd");
        efd.write(1).expect("write eventfd");
        let mut fds = [PollFd::new(efd.as_fd(), PollFlags::POLLIN)];
        let n = poll(&mut fds, PollTimeout::from(100u16)).expect("poll");
        assert!(n >= 1);
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
