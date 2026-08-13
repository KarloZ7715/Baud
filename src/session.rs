//! Una sesion = una terminal (PTY + Term + canal de input).

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::ansi::Term;
use crate::pty::PtyCommandSender;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

impl SessionId {
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        SessionId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Estado de una terminal individual dentro de la app.
pub struct Session {
    pub id: SessionId,
    pub term: Arc<Mutex<Term>>,
    pub pty_tx: PtyCommandSender,
    pub title: String,
    /// Output pendiente de redibujar cuando la sesion no esta enfocada.
    pub dirty: bool,
    /// Si es true, la sesion se mantiene abierta tras salir el proceso hijo.
    pub hold: bool,
    /// Si es true, la sesion se lanzo desde `-e` y sin `--hold` debe cerrarse al salir.
    pub close_on_exit: bool,
    /// Salida del PTY mientras la sesion no estaba enfocada (punto de actividad).
    pub has_activity: bool,
    /// Fd/handle duplicado del master, usado para sondear el proceso en
    /// primer plano desde el hilo de la GUI. `None` si no se pudo duplicar.
    pub foreground_probe: Option<crate::pty::foreground::Probe>,
    /// Ultimo (pgid, nombre) del proceso en primer plano resuelto por
    /// `crate::pty::foreground::poll`.
    pub foreground_cache: Option<(i32, String)>,
    /// Reset de vista pendiente tras input del usuario (ver
    /// `Term::apply_input_reset`). Lo activa `send_input` y lo consumen el
    /// hilo de drain o la GUI, segun quien consiga el lock del term.
    pub input_reset_pending: Arc<AtomicBool>,
    /// Puesta a `true` por la GUI en cada `send_input`; el hilo drain la
    /// consume con `swap(false)` para sacar ese redraw del throttle de
    /// `max_fps`. Mismo patron que `input_reset_pending`.
    pub echo_pending: Arc<AtomicBool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_incrementa_y_es_unico() {
        let a = SessionId::next();
        let b = SessionId::next();
        assert_ne!(a, b);
    }
}
