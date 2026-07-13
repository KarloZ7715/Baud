//! Event loop central de Baud.
//!
//! Sigue ADR-0005: dos hilos (GUI + PTY sincronico), sin async runtime.
//! El Term se comparte entre el hilo drain y la GUI via Arc<Mutex<Term>>.
//! El hilo drain envía UserEvent::RedrawNeeded al GUI vía EventLoopProxy.

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ansi::Term;
use crate::config::Config;
use crate::diagnostics::consent::ConsentState;
use crate::grid::{DEFAULT_COLS, DEFAULT_ROWS};
use crate::pty::{self, PtyCommand, PtyCommandSender, SessionBackend, WakeSource};
use crate::session::{Session, SessionId};
use crate::watchdog::EventLoopWatchdog;
use crate::window::{App, SessionHost, UserEvent};
use winit::event_loop::EventLoop;

#[cfg(unix)]
use std::os::fd::AsFd;

#[cfg(unix)]
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};

const METRICS_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// DSN por defecto del proyecto Baud en Sentry. Clave pública de cliente:
/// solo permite enviar eventos, no leerlos. El usuario puede sobrescribirlo
/// en config.toml o vía `BAUD_SENTRY_DSN` al compilar.
const DEFAULT_SENTRY_DSN: &str =
    "https://8def52da66d47b762219ac50c5f81d07@o4511043905126400.ingest.us.sentry.io/4511725171376128";

/// Resuelve el DSN de Sentry: override de config > variable de build > default del proyecto.
pub fn resolve_dsn(config: &Config) -> Option<String> {
    config
        .diagnostics
        .reporting
        .dsn
        .clone()
        .or_else(|| option_env!("BAUD_SENTRY_DSN").map(String::from))
        .or_else(|| Some(DEFAULT_SENTRY_DSN.to_string()))
}

/// Crea y registra el reporter si el consentimiento es `Accepted` y hay DSN.
fn init_reporter_if_accepted(config: &Config) {
    let consent = ConsentState::from_config(config.diagnostics.reporting.enabled);
    if consent != ConsentState::Accepted {
        tracing::info!("reporter: consent = {:?}, no network", consent);
        return;
    }

    let dsn = resolve_dsn(config);
    let Some(dsn) = dsn else {
        tracing::info!("reporter: consent accepted but no DSN — noop mode");
        return;
    };

    let install_id = crate::diagnostics::install_id::load_or_create_install_id();
    let transport = crate::diagnostics::transport::UreqTransport::new();
    let reporter =
        crate::diagnostics::reporter::Reporter::new(Some(dsn), install_id, Box::new(transport));
    crate::diagnostics::hooks::set_reporter(reporter.handle());
    tracing::info!("reporter: active, sending to Sentry");
}

// ponytail: tope de bytes por pasada del drain; suelta el mutex del Term para la GUI.
const DRAIN_MAX_BYTES_PER_PASS: usize = 256 * 1024;

/// Sesion cuyo cursor/celdas SGR 5 deben parpadear (solo una a la vez).
#[derive(Debug)]
pub struct BlinkFocus {
    current: Mutex<SessionId>,
}

impl BlinkFocus {
    pub fn new(id: SessionId) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new(id),
        })
    }

    pub fn set(&self, id: SessionId) {
        if let Ok(mut guard) = self.current.lock() {
            *guard = id;
        }
    }

    pub fn is_active(&self, id: SessionId) -> bool {
        self.current.lock().is_ok_and(|guard| *guard == id)
    }
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

/// Retorna true si pasó el intervalo mínimo desde el último redraw.
/// `interval_nanos = 0` desactiva el límite.
pub(crate) fn should_redraw(last: Instant, now: Instant, interval_nanos: u64) -> bool {
    if interval_nanos == 0 {
        return true;
    }
    now.duration_since(last).as_nanos() >= interval_nanos as u128
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

fn process_pty_commands(master: &mut pty::Pty, rx_gui_to_pty: &mpsc::Receiver<PtyCommand>) -> bool {
    let mut shutdown = false;
    while let Ok(cmd) = rx_gui_to_pty.try_recv() {
        match cmd {
            PtyCommand::Input(bytes) => {
                tracing::trace!("pty_thread: write {} bytes: {:02x?}", bytes.len(), bytes);
                let _ = master.write_input(&bytes);
            }
            PtyCommand::Resize { rows, cols } => {
                if let Err(e) = master.resize(rows, cols) {
                    tracing::warn!("error setting winsize: {e}");
                }
            }
            PtyCommand::Interrupt => {
                if let Err(e) = master.interrupt() {
                    tracing::warn!("error interrupting session: {e}");
                }
            }
            PtyCommand::Shutdown => {
                let sent = master.shutdown_graceful();
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

#[cfg(unix)]
fn master_poll_ready(revents: PollFlags) -> bool {
    revents.intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR)
}

enum ReadMasterOutcome {
    Done,
    Eof,
    DrainClosed,
    IoError,
}

/// Lee salida disponible en un buffer reutilizable y emite un `Output` coalescido.
///
/// Evita un `to_vec` por chunk desde un buffer de pila: los bytes se acumulan
/// en `out` y la propiedad se mueve al canal una sola vez.
fn read_master_available(
    master: &mut pty::Pty,
    scratch: &mut [u8],
    out: &mut Vec<u8>,
    tx_pty_to_gui: &mpsc::Sender<PtyEvent>,
) -> ReadMasterOutcome {
    out.clear();
    loop {
        match master.read_output(scratch) {
            Ok(0) => {
                if !out.is_empty() {
                    let chunk = std::mem::take(out);
                    if tx_pty_to_gui.send(PtyEvent::Output(chunk)).is_err() {
                        return ReadMasterOutcome::DrainClosed;
                    }
                }
                return ReadMasterOutcome::Eof;
            }
            Ok(n) => {
                tracing::trace!(
                    "pty_thread: read {} bytes: {:02x?}",
                    n,
                    &scratch[..n.min(40)]
                );
                out.extend_from_slice(&scratch[..n]);
                // Tope alineado con el drain para no acumular ráfagas gigantes.
                if out.len() >= DRAIN_MAX_BYTES_PER_PASS {
                    let chunk = std::mem::take(out);
                    out.reserve(scratch.len());
                    if tx_pty_to_gui.send(PtyEvent::Output(chunk)).is_err() {
                        return ReadMasterOutcome::DrainClosed;
                    }
                    return ReadMasterOutcome::Done;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if out.is_empty() {
                    return ReadMasterOutcome::Done;
                }
                let chunk = std::mem::take(out);
                out.reserve(scratch.len());
                if tx_pty_to_gui.send(PtyEvent::Output(chunk)).is_err() {
                    return ReadMasterOutcome::DrainClosed;
                }
                return ReadMasterOutcome::Done;
            }
            Err(e) => {
                tracing::warn!("error de I/O en PTY: {e}");
                let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                return ReadMasterOutcome::IoError;
            }
        }
    }
}

/// Lanza el hilo periodico de parpadeo y de recuperacion de sync.
///
/// Cada `blink_interval/2` (o ~50ms si hay sync activo o el parpadeo esta
/// desactivado) consulta el term: si hay cursor/celdas SGR 5 que parpadean, o
/// un frame sincronizado cuyo timeout de seguridad ya vencio, marca dirty y
/// envia `RedrawNeeded`. El wake de sync no exige foco, para que panes en
/// segundo plano tambien salgan del deferral tras el timeout.
pub(crate) fn spawn_blink_timer(
    term: Arc<Mutex<Term>>,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    session_id: SessionId,
    focus: Arc<BlinkFocus>,
) {
    thread::spawn(move || loop {
        let (interval_ms, sync_active) = match term.try_lock() {
            Ok(g) => (g.blink_interval_ms, g.sync_update_active),
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        // Con sync activo o blink desactivado, sondear al menos cada ~50ms.
        let sleep_for = if sync_active || interval_ms == 0 {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(interval_ms) / 2
        };
        thread::sleep(sleep_for);
        let need_redraw = match term.try_lock() {
            Ok(mut g) => {
                let sync_wake = g.sync_update_active && !g.should_defer_redraw();
                let blink_wake = focus.is_active(session_id) && g.has_blink_stuff();
                if sync_wake || blink_wake {
                    g.mark_dirty();
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        };
        if need_redraw {
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
    redraw_interval_nanos: Arc<AtomicU64>,
) -> std::io::Result<SpawnedSession> {
    let process_cfg = cfg.process_config();
    let session_id = SessionId::next();

    let master = pty::spawn_with(&process_cfg)?;
    master.set_winsize(rows, cols)?;

    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<PtyCommand>();

    #[cfg(unix)]
    let wakeup = Arc::new(pty::EventFdWake::new()?);
    #[cfg(windows)]
    let wakeup = Arc::new(pty::ConPtyWake::new()?);

    let cmd_sender =
        PtyCommandSender::new(tx_gui_to_pty, Arc::clone(&wakeup) as Arc<dyn WakeSource>);
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
            let interval_nanos = redraw_interval_nanos.load(Ordering::Relaxed);
            if pending_redraw && should_redraw(last_redraw, Instant::now(), interval_nanos) {
                send_redraw(&proxy_for_drain, session_id);
                metrics.record_redraw();
                last_redraw = Instant::now();
                pending_redraw = false;
            }

            let timeout = if pending_redraw {
                if interval_nanos == 0 {
                    Duration::ZERO
                } else {
                    Duration::from_nanos(interval_nanos).saturating_sub(last_redraw.elapsed())
                }
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

            let (response, title, clipboard_pending, clipboard_writes, total_bytes) = {
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
                let clipboard_writes = term_guard.take_clipboard_writes();
                (
                    response,
                    title,
                    clipboard_pending,
                    clipboard_writes,
                    total_bytes,
                )
            };
            metrics.record_bytes(total_bytes);

            for (text, primary) in clipboard_writes {
                crate::clipboard::set_detached(text, primary);
            }

            if !response.is_empty() {
                if let Err(e) = tx_response.send(PtyCommand::Input(response)) {
                    tracing::warn!("drain: could not forward PTY response ({e}); query discarded");
                }
            }
            send_title_and_clipboard(&proxy_for_drain, session_id, title, clipboard_pending);

            let interval_nanos = redraw_interval_nanos.load(Ordering::Relaxed);
            if should_redraw(last_redraw, Instant::now(), interval_nanos) {
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
        pty_thread_main(master, wakeup_pty, rx_gui_to_pty, tx_pty_to_gui);
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

#[cfg(unix)]
fn pty_thread_main(
    mut master: pty::Pty,
    wakeup_pty: Arc<pty::EventFdWake>,
    rx_gui_to_pty: mpsc::Receiver<PtyCommand>,
    tx_pty_to_gui: mpsc::Sender<PtyEvent>,
) {
    let mut scratch = [0u8; 4096];
    let mut out_buf = Vec::with_capacity(4096);

    if let Err(e) = master.set_nonblocking() {
        tracing::warn!("could not set PTY to non-blocking: {e}");
    }

    loop {
        let (master_ready, wakeup_ready) = {
            let mut poll_fds = [
                PollFd::new(master.as_fd(), PollFlags::POLLIN),
                PollFd::new(wakeup_pty.eventfd().as_fd(), PollFlags::POLLIN),
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
            wakeup_pty.drain();
            if process_pty_commands(&mut master, &rx_gui_to_pty) {
                return;
            }
        }

        if master_ready {
            match read_master_available(&mut master, &mut scratch, &mut out_buf, &tx_pty_to_gui) {
                ReadMasterOutcome::Done => {}
                ReadMasterOutcome::Eof => {
                    let _ = tx_pty_to_gui.send(PtyEvent::Exited(-1));
                    return;
                }
                ReadMasterOutcome::DrainClosed | ReadMasterOutcome::IoError => return,
            }
        }
    }
}

#[cfg(windows)]
fn pty_thread_main(
    mut master: pty::Pty,
    wakeup_pty: Arc<pty::ConPtyWake>,
    rx_gui_to_pty: mpsc::Receiver<PtyCommand>,
    tx_pty_to_gui: mpsc::Sender<PtyEvent>,
) {
    let mut scratch = [0u8; 4096];
    let mut out_buf = Vec::with_capacity(4096);

    if let Err(e) = master.set_nonblocking() {
        tracing::warn!("could not set PTY to non-blocking: {e}");
    }

    loop {
        let ready = match master.wait_ready(&wakeup_pty) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("wait en hilo PTY fallo: {e}");
                let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                return;
            }
        };

        if ready.wake {
            wakeup_pty.drain();
            if process_pty_commands(&mut master, &rx_gui_to_pty) {
                return;
            }
        }

        if ready.output {
            match read_master_available(&mut master, &mut scratch, &mut out_buf, &tx_pty_to_gui) {
                ReadMasterOutcome::Done => {}
                ReadMasterOutcome::Eof => {
                    let _ = tx_pty_to_gui.send(PtyEvent::Exited(-1));
                    return;
                }
                ReadMasterOutcome::DrainClosed | ReadMasterOutcome::IoError => return,
            }
        }
    }
}

/// Punto de entrada del event loop.
///
/// Crea el PTY, lanza el shell configurado, y arranca los hilos necesarios.
/// Retorna cuando se cierra la ventana (event_loop.exit()).
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let load_result = Config::load();
    let app_config = load_result.config;
    let process_cfg = app_config.process_config();
    let startup_command = process_cfg.startup_command.clone();

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let redraw_interval_nanos = Arc::new(AtomicU64::new(app_config.render.redraw_interval_nanos()));

    let spawned = spawn_session(
        &app_config,
        DEFAULT_ROWS as u16,
        DEFAULT_COLS as u16,
        proxy.clone(),
        Arc::clone(&redraw_interval_nanos),
    )?;

    let blink_focus = BlinkFocus::new(spawned.session.id);

    spawn_blink_timer(
        Arc::clone(&spawned.session.term),
        proxy.clone(),
        spawned.session.id,
        Arc::clone(&blink_focus),
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

    let watchdog = EventLoopWatchdog::spawn_if(app_config.diagnostics.watchdog);
    if app_config.diagnostics.watchdog {
        tracing::info!(
            "event loop watchdog active (handler telemetry, stall={}s)",
            2
        );
    }

    // Inicializar reporter de errores si el consentimiento ya está aceptado.
    init_reporter_if_accepted(&app_config);

    let mut app = App::new(
        vec![SessionHost::from_spawned(spawned)],
        app_config,
        config_watch,
        Some(proxy),
        blink_focus,
        load_result.source,
        watchdog,
    );
    app.set_redraw_interval_handle(redraw_interval_nanos);

    if let Some(cmd) = startup_command {
        app.send_startup_input(format!("{cmd}\n").into_bytes());
    }

    tracing::info!("event loop started, shell running in PTY");

    event_loop.run_app(&mut app)?;

    app.join_session_threads();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::io::Read;
    #[cfg(unix)]
    use std::os::fd::AsFd;

    #[cfg(unix)]
    use nix::poll::PollTimeout;
    #[cfg(unix)]
    use nix::poll::{poll, PollFd, PollFlags};

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
    fn test_dynamic_redraw_interval() {
        let t0 = Instant::now();
        let fps60 = Duration::from_secs_f64(1.0 / 60.0).as_nanos() as u64;
        let fps120 = Duration::from_secs_f64(1.0 / 120.0).as_nanos() as u64;
        assert!(!should_redraw(t0, t0 + Duration::from_millis(5), fps60));
        assert!(should_redraw(t0, t0 + Duration::from_millis(20), fps60));
        assert!(should_redraw(t0, t0 + Duration::from_millis(9), fps120));
        assert!(should_redraw(t0, t0 + Duration::from_millis(1), 0));
    }

    #[cfg(unix)]
    #[test]
    fn test_eventfd_despierta_poll() {
        let wake = pty::EventFdWake::new().expect("eventfd");
        wake.wake();
        let mut fds = [PollFd::new(wake.eventfd().as_fd(), PollFlags::POLLIN)];
        let n = poll(&mut fds, PollTimeout::from(100u16)).expect("poll");
        assert!(n >= 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_set_winsize_after_spawn() {
        let master = pty::spawn("bash", &["-c", "exit"]).expect("spawn fallo");
        assert!(master.set_winsize(24, 80).is_ok());
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn test_read_master_coalesces_without_per_chunk_to_vec() {
        let (tx, rx) = mpsc::channel::<PtyEvent>();
        let mut master = pty::spawn("bash", &["-c", "true"]).expect("spawn");
        master.set_nonblocking().expect("nonblock");
        let mut scratch = [0u8; 64];
        let mut out = Vec::with_capacity(64);
        match read_master_available(&mut master, &mut scratch, &mut out, &tx) {
            ReadMasterOutcome::Done
            | ReadMasterOutcome::Eof
            | ReadMasterOutcome::DrainClosed
            | ReadMasterOutcome::IoError => {}
        }
        while let Ok(ev) = rx.try_recv() {
            if let PtyEvent::Output(bytes) = ev {
                assert!(!bytes.is_empty());
            }
        }
    }
}
