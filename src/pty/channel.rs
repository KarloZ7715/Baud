//! Canal GUI → hilo PTY (comandos + wakeup).

use std::sync::{mpsc, Arc};

use super::contract::WakeSource;

/// Comandos del GUI al hilo PTY.
pub enum PtyCommand {
    /// Bytes de input para escribir al master.
    Input(Vec<u8>),
    /// Redimensionar: el child debe actualizar su winsize.
    Resize { rows: u16, cols: u16 },
    /// Semántica de Ctrl+C (el backend escribe `0x03`).
    Interrupt,
    /// Apagado: pedir salida ordenada al child.
    Shutdown,
}

/// Envía comandos al hilo PTY y despierta el wait del backend.
#[derive(Clone)]
pub struct PtyCommandSender {
    tx: mpsc::Sender<PtyCommand>,
    wakeup: Arc<dyn WakeSource>,
}

impl PtyCommandSender {
    pub fn new(tx: mpsc::Sender<PtyCommand>, wakeup: Arc<dyn WakeSource>) -> Self {
        Self { tx, wakeup }
    }

    pub fn send(&self, cmd: PtyCommand) -> Result<(), mpsc::SendError<PtyCommand>> {
        self.tx.send(cmd)?;
        self.wakeup.wake();
        Ok(())
    }

    #[cfg(test)]
    pub fn new_for_test(tx: mpsc::Sender<PtyCommand>, wakeup: Arc<dyn WakeSource>) -> Self {
        Self { tx, wakeup }
    }
}
