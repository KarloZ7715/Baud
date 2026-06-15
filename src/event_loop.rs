//! Event loop central de Baud.
//!
//! Fase 0: hilo PTY lee del master y envia bytes por mpsc.
//! Un hilo placeholder loguea esos bytes con tracing.
//! El canal GUI->PTY existe pero no se alimenta (Fase 1).

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

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
            // ponytail: en Fase 1 esto se alimenta con input real del teclado.
            while let Ok(bytes) = rx_gui_to_pty.try_recv() {
                let _ = master.write_all(&bytes); // ponytail: ignorar errores en Fase 0
            }
        }
    });

    // Hilo placeholder: drena el canal y loguea bytes. NO renderea nada.
    // ponytail: en Fase 2 esto se reemplaza por el render real via winit.
    let drain_handle = thread::spawn(move || {
        while let Ok(event) = rx_pty_to_gui.recv() {
            match event {
                PtyEvent::Output(bytes) => {
                    tracing::info!(bytes_len = bytes.len(), "pty output");
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
