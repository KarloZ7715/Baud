//! Contrato de I/O de sesión propiedad de Baud.
//!
//! El GUI se queda en [`crate::pty::PtyCommand`] / eventos. Los backends
//! implementan este trait; el hilo PTY posee el bucle wait/read.

use std::io;

use super::ProcessConfig;

/// Operaciones de I/O de sesión compartidas por backends Unix y Windows.
pub trait SessionBackend: Send {
    /// Lanza el proceso configurado asociado a esta sesión.
    fn spawn(cfg: &ProcessConfig) -> io::Result<Self>
    where
        Self: Sized;

    /// Escribe bytes a la entrada de la sesión (teclado / pegado / respuestas).
    fn write_input(&mut self, data: &[u8]) -> io::Result<()>;

    /// Actualiza el winsize del hijo / tamaño de la pseudo-consola.
    fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()>;

    /// Entrega semántica de Ctrl+C (byte `0x03` en la entrada de la sesión).
    fn interrupt(&mut self) -> io::Result<()>;

    /// Pide al hijo que salga de forma ordenada. Indica si se envió la señal/petición.
    fn shutdown_graceful(&mut self) -> bool;

    /// Fuerza la terminación del hijo (red de seguridad / ruta de Drop).
    fn force_kill(&mut self);

    /// Lee salida disponible en `buf`. Mismo contrato que [`std::io::Read::read`].
    fn read_output(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Pone el lado de salida en modo non-blocking si la plataforma lo soporta.
    fn set_nonblocking(&mut self) -> io::Result<()>;
}

/// Despertador entre hilos para interrumpir un wait bloqueante en el hilo PTY.
pub trait WakeSource: Send + Sync {
    fn wake(&self);
    fn drain(&self);
}
