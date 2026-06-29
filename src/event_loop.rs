//! Event loop central de Baud.
//!
//! Sigue ADR-0005: dos hilos (GUI + PTY sincronico), sin async runtime.
//! El Term se comparte entre el hilo drain y la GUI via Arc<Mutex<Term>>.
//! El hilo drain envía UserEvent::RedrawNeeded al GUI vía EventLoopProxy.

use std::io::{ErrorKind, Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::ansi::Term;
use crate::grid::{DEFAULT_COLS, DEFAULT_ROWS};
use crate::pty;
use crate::window::{App, UserEvent};
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use winit::event_loop::EventLoop;

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

/// Punto de entrada del event loop.
///
/// Crea el PTY, lanza bash, y arranca los hilos necesarios.
/// Retorna cuando se cierra la ventana (event_loop.exit()).
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // ponytail: spawn bash interactivo. -i fuerza prompt interactivo.
    let master = pty::spawn("bash", &["-i"])?;
    master.set_winsize(DEFAULT_ROWS as u16, DEFAULT_COLS as u16)?;

    // Dos canales separados: PTY->drain y GUI->PTY
    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<PtyCommand>();
    let tx_response = tx_gui_to_pty.clone();

    // Term compartido entre hilo drain y App (GUI).
    let term = Arc::new(Mutex::new(Term::new()));

    // Hilo drain: alimenta el parser, comparte el term via Arc<Mutex<Term>>,
    // envía tick al GUI loop por event_loop_proxy.
    let term_drain = Arc::clone(&term);
    let proxy_for_drain = Arc::new(Mutex::new(
        None::<winit::event_loop::EventLoopProxy<UserEvent>>,
    ));
    let proxy_for_drain_clone = Arc::clone(&proxy_for_drain);

    let drain_handle = thread::spawn(move || {
        let mut parser = vte::Parser::new();
        while let Ok(event) = rx_pty_to_gui.recv() {
            match event {
                PtyEvent::Output(bytes) => {
                    let response = {
                        let mut term_guard =
                            term_drain.lock().expect("term mutex poisoned en drain");
                        parser.advance(&mut *term_guard, &bytes);
                        term_guard.mark_dirty();
                        term_guard.take_pty_response()
                    };
                    if !response.is_empty() {
                        let _ = tx_response.send(PtyCommand::Input(response));
                    }
                    tracing::trace!(
                        "drain: processed {} bytes: {:02x?}, sending RedrawNeeded",
                        bytes.len(),
                        &bytes[..bytes.len().min(40)]
                    );
                    // Envia tick al GUI loop para que redibuje.
                    if let Some(proxy) = proxy_for_drain_clone
                        .lock()
                        .expect("proxy mutex poisoned en drain")
                        .as_ref()
                    {
                        let _ = proxy.send_event(UserEvent::RedrawNeeded);
                    }
                }
                PtyEvent::Exited(code) => {
                    tracing::info!("child termino con codigo {code}");
                    if let Some(proxy) = proxy_for_drain_clone
                        .lock()
                        .expect("proxy mutex poisoned en drain")
                        .as_ref()
                    {
                        let _ = proxy.send_event(UserEvent::PtyExited(code));
                    }
                }
                PtyEvent::IoError(msg) => {
                    tracing::warn!("error de I/O del PTY: {msg}");
                    if let Some(proxy) = proxy_for_drain_clone
                        .lock()
                        .expect("proxy mutex poisoned en drain")
                        .as_ref()
                    {
                        let _ = proxy.send_event(UserEvent::PtyError(msg));
                    }
                }
            }
        }
    });

    // Crear event loop con soporte para UserEvent.
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    // Guardar el proxy en el slot compartido para que el drain lo use.
    *proxy_for_drain
        .lock()
        .expect("proxy mutex poisoned al setear") = Some(proxy);

    let pty_tx = Arc::new(Mutex::new(Some(tx_gui_to_pty)));

    let mut app = App::new(Arc::clone(&term), Arc::clone(&pty_tx));

    // Hilo PTY: lee del master, envía por tx_pty_to_gui.
    // También recibe comandos de rx_gui_to_pty (Input, Resize, Shutdown).
    let pty_thread = thread::spawn(move || {
        let mut master = master;
        let mut buf = [0u8; 4096];

        // Configurar el master fd como no-bloqueante para evitar deadlock:
        // el hilo PTY debe poder procesar comandos del GUI incluso cuando
        // no hay datos disponibles del master (bash esperando input).
        // ponytail: non-blocking + sleep(1ms) es mas simple que poll/mio.
        if let Ok(flags) = fcntl(master.fd(), FcntlArg::F_GETFL) {
            let nonblock = OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK;
            let _ = fcntl(master.fd(), FcntlArg::F_SETFL(nonblock));
        }

        loop {
            let mut had_activity = false;

            // Leer todos los datos disponibles del master (no-bloqueante).
            loop {
                match master.read(&mut buf) {
                    Ok(0) => {
                        // EOF: child termino.
                        let _ = tx_pty_to_gui.send(PtyEvent::Exited(-1));
                        return;
                    }
                    Ok(n) => {
                        had_activity = true;
                        let data = PtyEvent::Output(buf[..n].to_vec());
                        tracing::trace!("pty_thread: read {} bytes: {:02x?}", n, &buf[..n.min(40)]);
                        if tx_pty_to_gui.send(data).is_err() {
                            return; // drain cerro el canal
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break, // no more data
                    Err(e) => {
                        tracing::warn!("error de I/O en PTY: {e}");
                        let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                        return;
                    }
                }
            }

            // Procesar comandos pendientes del GUI (Input, Resize, Shutdown).
            while let Ok(cmd) = rx_gui_to_pty.try_recv() {
                had_activity = true;
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
                        return;
                    }
                }
            }

            // Si no hubo actividad, dormir 1ms para evitar spin-loop.
            if !had_activity {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });

    tracing::info!("event loop iniciado, bash corriendo en PTY");

    // Lanzar GUI loop (bloqueante hasta cerrar ventana).
    event_loop.run_app(&mut app)?;

    // Al salir del event loop, deja que pty_thread y drain terminen.
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
    fn test_set_winsize_after_spawn() {
        // verificar que set_winsize funciona inmediatamente despues de spawn.
        // Sin el fix, el PTY se crea con winsize={0,0,0,0} y bash usa fallback de 80 cols.
        let master = pty::spawn("bash", &["-c", "exit"]).expect("spawn fallo");
        assert!(master.set_winsize(24, 80).is_ok());
    }

    #[test]
    fn test_pty_eof_no_panic() {
        // Spawn bash que termina inmediatamente. La lectura del master
        // debe retornar Ok(0) (EOF) sin panic.
        let mut master = pty::spawn("bash", &["-c", "exit"]).expect("spawn fallo");
        let mut buf = [0u8; 4096];
        // Leer hasta EOF. bash termina rapido, asi que read retorna Ok(0).
        loop {
            match master.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
}
