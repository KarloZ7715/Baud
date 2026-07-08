//! Canal GUI → hilo PTY (comandos + wakeup via eventfd).

use std::sync::{mpsc, Arc};

use nix::sys::eventfd::EventFd;

/// Comandos del GUI al hilo PTY.
pub enum PtyCommand {
    /// Bytes de input para escribir al master.
    Input(Vec<u8>),
    /// Resize: el child debe actualizar su winsize.
    Resize { rows: u16, cols: u16 },
    /// Shutdown: enviar SIGHUP al child y esperar.
    Shutdown,
}

/// Envia comandos al hilo PTY y despierta `poll` via eventfd.
#[derive(Clone)]
pub struct PtyCommandSender {
    tx: mpsc::Sender<PtyCommand>,
    wakeup: Arc<EventFd>,
}

impl PtyCommandSender {
    pub fn new(tx: mpsc::Sender<PtyCommand>, wakeup: Arc<EventFd>) -> Self {
        Self { tx, wakeup }
    }

    pub fn send(&self, cmd: PtyCommand) -> Result<(), mpsc::SendError<PtyCommand>> {
        self.tx.send(cmd)?;
        let _ = self.wakeup.write(1);
        Ok(())
    }

    #[cfg(test)]
    pub fn new_for_test(tx: mpsc::Sender<PtyCommand>, wakeup: Arc<EventFd>) -> Self {
        Self { tx, wakeup }
    }
}
