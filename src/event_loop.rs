//! Event loop central de Baud.
//!
//! Sigue ADR-0005: dos hilos (GUI + PTY sincronico), sin async runtime.
//! El Term se comparte entre el hilo drain y la GUI via Arc<Mutex<Term>>.
//! El hilo drain envía UserEvent::RedrawNeeded al GUI vía EventLoopProxy.

use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use crate::ansi::Term;
use crate::pty;
use crate::window::{App, UserEvent};
use winit::event_loop::EventLoop;

/// Comandos del GUI al hilo PTY.
pub enum PtyCommand {
    /// Bytes de input para escribir al master.
    Input(Vec<u8>),
    /// Resize: el child debe actualizar su winsize.
    Resize { rows: u16, cols: u16 },
    /// Shutdown: enviar SIGHUP al child y esperar. (Ronda 4)
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

    // Dos canales separados: PTY->drain y GUI->PTY
    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<PtyCommand>();

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
                    {
                        let mut term_guard =
                            term_drain.lock().expect("term mutex poisoned en drain");
                        parser.advance(&mut *term_guard, &bytes);
                    }
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
        loop {
            // Drenar comandos pendientes del GUI->PTY (no bloqueante).
            // ponytail: se procesan antes de la lectura bloqueante para que
            // Resize se aplique inmediatamente y Shutdown pueda interrumpir.
            while let Ok(cmd) = rx_gui_to_pty.try_recv() {
                match cmd {
                    PtyCommand::Input(bytes) => {
                        let _ = master.write_all(&bytes);
                    }
                    PtyCommand::Resize { rows, cols } => {
                        // ponytail: el ioctl se hace en el hilo PTY, no en el GUI,
                        // para evitar concurrencia con la lectura del master fd.
                        if let Err(e) = master.set_winsize(rows, cols) {
                            tracing::warn!("error al setear winsize: {e}");
                        }
                    }
                    PtyCommand::Shutdown => {
                        // Enviar SIGHUP al child para que haga cleanup.
                        let sent = master.send_sighup();
                        if sent {
                            // Esperar 100ms para que el child procese el SIGHUP.
                            // ponytail: el sleep esta en el hilo PTY, NO en el GUI, para
                            // evitar que el compositor (Hyprland) marque la ventana como
                            // "no responde" al cerrar.
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        // Salir del loop del pty_thread.
                        // ponytail: usamos return en vez de break para salir del closure.
                        return;
                    }
                }
            }

            // Leer datos del master (bloqueante).
            match master.read(&mut buf) {
                Ok(0) => {
                    // EOF: child termino. Enviar Exited al drain antes de salir.
                    let _ = tx_pty_to_gui.send(PtyEvent::Exited(-1));
                    break;
                }
                Ok(n) => {
                    let data = PtyEvent::Output(buf[..n].to_vec());
                    if tx_pty_to_gui.send(data).is_err() {
                        break; // drain cerro el canal
                    }
                }
                Err(e) => {
                    // Error de I/O: NO panic. Loguear y enviar IoError al drain.
                    tracing::warn!("error de I/O en PTY: {e}");
                    let _ = tx_pty_to_gui.send(PtyEvent::IoError(e.to_string()));
                    break;
                }
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
