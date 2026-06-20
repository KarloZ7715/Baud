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

/// Eventos que el hilo PTY envía al hilo drain.
pub enum PtyEvent {
    /// Datos crudos leidos del master PTY.
    Output(Vec<u8>),
    // ponytail: en fases futuras: Resized, Exited, etc.
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
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<Vec<u8>>();

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

    let mut app = App::new(Arc::clone(&term));

    // Hilo PTY: lee del master, envía por tx_pty_to_gui.
    // También recibe input de rx_gui_to_pty y lo escribe al master.
    let pty_thread = thread::spawn(move || {
        let mut master = master;
        let mut buf = [0u8; 4096];
        loop {
            // Leer datos del master (bloqueante).
            match master.read(&mut buf) {
                Ok(0) => break, // EOF: child termino
                Ok(n) => {
                    let data = PtyEvent::Output(buf[..n].to_vec());
                    if tx_pty_to_gui.send(data).is_err() {
                        break; // drain cerro el canal
                    }
                }
                Err(_) => break,
            }

            // Drenar input pendiente del canal GUI->PTY (no bloqueante).
            // ponytail: keyboard input real llega en Sprint 6.
            while let Ok(bytes) = rx_gui_to_pty.try_recv() {
                let _ = master.write_all(&bytes);
            }
        }
    });

    tracing::info!("event loop iniciado, bash corriendo en PTY");

    // Lanzar GUI loop (bloqueante hasta cerrar ventana).
    event_loop.run_app(&mut app)?;

    // Al salir del event loop, deja que pty_thread y drain terminen.
    drop(tx_gui_to_pty);
    drop(term);
    let _ = pty_thread.join();
    let _ = drain_handle.join();

    Ok(())
}
