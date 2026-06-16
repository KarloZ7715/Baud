//! Event loop central de Baud.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use crate::ansi::Term;
use crate::pty;

/// Eventos que el hilo PTY envia al hilo GUI.
pub enum PtyEvent {
    /// Datos crudos leidos del master PTY.
    Output(Vec<u8>),
    // ponytail: en fases futuras: Resized, Exited, etc.
}

/// Punto de entrada del event loop.
///
/// Crea el PTY, lanza bash, y arranca los hilos necesarios.
/// Retorna cuando el child termina (EOF en master).
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // ponytail: spawn bash interactivo. -i fuerza prompt interactivo.
    let master = pty::spawn("bash", &["-i"])?;

    // Dos canales separados: PTY->GUI y GUI->PTY
    let (tx_pty_to_gui, rx_pty_to_gui) = mpsc::channel::<PtyEvent>();
    let (tx_gui_to_pty, rx_gui_to_pty) = mpsc::channel::<Vec<u8>>();

    // Hilo PTY: lee del master, envia por tx_pty_to_gui.
    // Tambien recibe input de rx_gui_to_pty y lo escribe al master.
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
                        break; // GUI cerro el canal
                    }
                }
                Err(_) => break,
            }

            // Drenar input pendiente del canal GUI->PTY (no bloqueante).
            // ponytail: keyboard input real llega en Sprint 3 con el renderer.
            while let Ok(bytes) = rx_gui_to_pty.try_recv() {
                let _ = master.write_all(&bytes); // ponytail: error handling real en Sprint 4
            }
        }
    });

    // Hilo drain: alimenta el parser vte con bytes del PTY.
    // ponytail: el render visual via wgpu llega en Fase 2 (Sprint 3); este
    // hilo se mantiene, solo cambia el consumidor del estado de Term.
    let drain_handle = thread::spawn(move || {
        let mut parser = vte::Parser::new();
        let mut term = Term::new();

        while let Ok(event) = rx_pty_to_gui.recv() {
            match event {
                PtyEvent::Output(bytes) => {
                    parser.advance(&mut term, &bytes);
                }
            }
        }
    });

    tracing::info!("event loop iniciado, bash corriendo en PTY");

    // Esperar a que el hilo PTY termine (child muere).
    let _ = pty_thread.join();

    // Al cerrar el master, el hilo drain recibe disconnected y termina.
    drop(tx_gui_to_pty);
    let _ = drain_handle.join();

    Ok(())
}
