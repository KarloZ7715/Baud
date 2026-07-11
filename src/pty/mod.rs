pub mod channel;
pub mod config;
pub mod contract;

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;

pub use channel::{PtyCommand, PtyCommandSender};
pub use config::ProcessConfig;
pub use contract::{SessionBackend, WakeSource};

#[cfg(unix)]
pub use unix::{open, EventFdWake, Pty};

#[cfg(windows)]
pub use windows::{ConPtyWake, Pty};

use std::io;

/// Lanza un proceso con el shell y args dados.
pub fn spawn(shell: &str, args: &[&str]) -> io::Result<Pty> {
    #[cfg(unix)]
    {
        unix::spawn(shell, args).map_err(io::Error::from)
    }
    #[cfg(windows)]
    {
        windows::spawn(shell, args)
    }
}

/// Lanza según [`ProcessConfig`].
pub fn spawn_with(cfg: &ProcessConfig) -> io::Result<Pty> {
    #[cfg(unix)]
    {
        unix::spawn_with(cfg).map_err(io::Error::from)
    }
    #[cfg(windows)]
    {
        windows::spawn_with(cfg)
    }
}

/// Crea el wake source de plataforma para el canal de comandos PTY.
pub fn create_wake() -> io::Result<std::sync::Arc<dyn WakeSource>> {
    #[cfg(unix)]
    {
        Ok(std::sync::Arc::new(EventFdWake::new()?) as std::sync::Arc<dyn WakeSource>)
    }
    #[cfg(windows)]
    {
        Ok(std::sync::Arc::new(ConPtyWake::new()?) as std::sync::Arc<dyn WakeSource>)
    }
}
