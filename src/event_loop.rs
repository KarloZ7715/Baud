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
use crate::pty::{self, PtyCommand, PtyCommandSender};
use crate::session::{Session, SessionId};
use crate::window::{App, SessionHost, UserEvent};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nix::sys::eventfd::{EfdFlags, EventFd};
use winit::event_loop::EventLoop;

// ponytail: throttle a 16ms (~60fps); bajar intervalo si se quiere 120Hz.
const REDRAW_MIN_INTERVAL: Duration = Duration::from_millis(16);

const METRICS_LOG_INTERVAL: Duration = Duration::from_secs(5);

// ponytail: tope de bytes por pasada del drain; suelta el mutex del Term para la GUI.
const DRAIN_MAX_BYTES_PER_PASS: usize = 256 * 1024;

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

fn send_redraw(proxy: &winit::event_loop::EventLoopProxy<UserEvent>, session_id: SessionId) {
    let _ = proxy.send_event(UserEvent::RedrawNeeded(session_id));
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
    proxy: &winit::event_loop::EventLoopProxy<UserEvent>,
    session_id: SessionId,
    title: Option<String>,
    clipboard_pending: Option<(u8, bool)>,
) {
    if let Some(t) = title {
        let _ = proxy.send_event(UserEvent::SetTitle(session_id, t));
    }
    if let Some((target, bell)) = clipboard_pending {
        let _ = proxy.send_event(UserEvent::ReadClipboard(session_id, target, bell));
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
pub(crate) fn spawn_blink_timer(
    term: Arc<Mutex<Term>>,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    session_id: SessionId,
) {
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
            let _ = proxy.send_event(UserEvent::RedrawNeeded(session_id));
        }
    });
}

fn handle_non_output_pty_event(
    event: PtyEvent,
    proxy: &winit::event_loop::EventLoopProxy<UserEvent>,
    session_id: SessionId,
) {
    match event {
        PtyEvent::Exited(code) => {
            tracing::info!("child termino con codigo {code}");
            let _ = proxy.send_event(UserEvent::PtyExited(session_id, code));
        }
        PtyEvent::IoError(msg) => {
            tracing::warn!("error de I/O del PTY: {msg}");
            let _ = proxy.send_event(UserEvent::PtyError(session_id, msg));
        }
        PtyEvent::Output(_) => unreachable!("Output se maneja en el match principal"),
    }
}

/// Recursos de una sesion recien creada (PTY, hilos drain/pty).
pub struct SpawnedSession {
    pub session: Session,
    pub drain_handle: thread::JoinHandle<()>,
    pub pty_handle: thread::JoinHandle<()>,
}

/// Crea una sesion completa: PTY, Term, hilos drain y pty.
///
/// Los eventos del drain y del PTY se etiquetan con el `SessionId` de la sesion.
pub fn spawn_session(
    cfg: &Config,
    rows: u16,
    cols: u16,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
) -> std::io::Result<SpawnedSession> {
    let process_cfg = cfg.process_config();
    let session_id = SessionId::next();

    let master = pty::spawn_with(&process_cfg)?;
    master.set_winsize(rows, cols)?;

    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<PtyCommand>();
    let wakeup =
        Arc::new(EventFd::from_flags(EfdFlags::EFD_NONBLOCK).expect("no se pudo crear eventfd"));
    let cmd_sender = PtyCommandSender::new(tx_gui_to_pty, Arc::clone(&wakeup));
    let tx_response = cmd_sender.clone();

    let rows_usize = rows as usize;
    let cols_usize = cols as usize;
    let term = Arc::new(Mutex::new({
        let mut t = Term::new_sized(rows_usize, cols_usize, cfg.scrollback_max_lines());
        cfg.apply_to_term(&mut t);
        t
    }));

    let term_drain = Arc::clone(&term);
    let proxy_for_drain = proxy.clone();

    let drain_handle = thread::spawn(move || {
        let mut parser = vte::Parser::new();
        let mut metrics = DrainMetrics::new();
        let mut last_redraw = Instant::now();
        let mut pending_redraw = false;
        let mut output_backlog: VecDeque<Vec<u8>> = VecDeque::new();

        loop {
            if pending_redraw && should_redraw(last_redraw, Instant::now()) {
                send_redraw(&proxy_for_drain, session_id);
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
                        handle_non_output_pty_event(other, &proxy_for_drain, session_id);
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
                term_guard.search_refresh_if_active();
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
            send_title_and_clipboard(&proxy_for_drain, session_id, title, clipboard_pending);

            if should_redraw(last_redraw, Instant::now()) {
                send_redraw(&proxy_for_drain, session_id);
                metrics.record_redraw();
                last_redraw = Instant::now();
                pending_redraw = false;
            } else {
                pending_redraw = true;
            }

            for event in deferred {
                match event {
                    PtyEvent::Output(bytes) => output_backlog.push_back(bytes),
                    other => handle_non_output_pty_event(other, &proxy_for_drain, session_id),
                }
            }

            metrics.maybe_log();
        }
    });

    let wakeup_pty = Arc::clone(&wakeup);
    let pty_handle = thread::spawn(move || {
        let mut master = master;
        let mut buf = [0u8; 4096];

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

    let session = Session {
        id: session_id,
        term,
        pty_tx: cmd_sender,
        title: String::new(),
        dirty: false,
    };

    Ok(SpawnedSession {
        session,
        drain_handle,
        pty_handle,
    })
}

/// Punto de entrada del event loop.
///
/// Crea el PTY, lanza el shell configurado, y arranca los hilos necesarios.
/// Retorna cuando se cierra la ventana (event_loop.exit()).
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app_config = Config::load();
    let process_cfg = app_config.process_config();
    let startup_command = process_cfg.startup_command.clone();

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let spawned = spawn_session(
        &app_config,
        DEFAULT_ROWS as u16,
        DEFAULT_COLS as u16,
        proxy.clone(),
    )?;

    spawn_blink_timer(
        Arc::clone(&spawned.session.term),
        proxy.clone(),
        spawned.session.id,
    );

    let config_watch = Arc::new(Mutex::new(crate::config::watch::WatchState::new(
        crate::config::watch::config_mtime(),
    )));
    let watch_for_thread = Arc::clone(&config_watch);
    let proxy_cfg = proxy.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(1000));
        let now = crate::config::watch::config_mtime();
        if let Ok(mut state) = watch_for_thread.lock() {
            if state.changed(now) {
                match Config::try_load_from_disk() {
                    Ok(cfg) => {
                        let _ = proxy_cfg.send_event(UserEvent::ConfigReloaded(Box::new(cfg)));
                    }
                    Err(msg) => {
                        let _ = proxy_cfg.send_event(UserEvent::ConfigReloadFailed(msg));
                    }
                }
            }
        }
    });

    let mut app = App::new(
        vec![SessionHost::from_spawned(spawned)],
        app_config,
        config_watch,
        Some(proxy),
    );

    if let Some(cmd) = startup_command {
        app.send_startup_input(format!("{cmd}\n").into_bytes());
    }

    tracing::info!("event loop iniciado, shell corriendo en PTY");

    event_loop.run_app(&mut app)?;

    app.join_session_threads();

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
