//! Deteccion de bloqueos del hilo GUI (event loop sin heartbeat).

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Avisa si el event loop deja de enviar heartbeats durante demasiado tiempo.
#[derive(Clone)]
pub struct EventLoopWatchdog {
    tx: mpsc::Sender<()>,
}

impl EventLoopWatchdog {
    /// Lanza el hilo de vigilancia. Debe llamarse una vez al arrancar la app.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("baud-watchdog".into())
            .spawn(move || loop {
                thread::sleep(HEARTBEAT_INTERVAL);
                if rx.try_recv().is_err() {
                    tracing::warn!(
                        target: "baud::watchdog",
                        "event loop sin heartbeat en {:?} — posible bloqueo (GPU, mutex, I/O)",
                        HEARTBEAT_INTERVAL
                    );
                }
                while rx.try_recv().is_ok() {}
            })
            .expect("no se pudo iniciar watchdog");
        Self { tx }
    }

    /// Llamar desde `about_to_wait` en cada iteracion del event loop.
    pub fn ping(&self) {
        let _ = self.tx.send(());
    }

    /// Instancia sin hilo de vigilancia (tests).
    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::channel();
        Self { tx }
    }
}
