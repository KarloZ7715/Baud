//! Hooks globales: panic hook y capa de tracing para enviar errores al reporter.
//!
//! El panic hook se instala al inicio (antes de cargar la config) y usa un
//! `OnceLock<ReporterHandle>` que se rellena cuando el reporter está listo.

use std::sync::OnceLock;

use crate::diagnostics::reporter::{EventLevel, ReportEvent, ReporterHandle};

/// Handle global del reporter, accesible desde el panic hook.
static REPORTER: OnceLock<ReporterHandle> = OnceLock::new();

/// Registra el handle del reporter para que el panic hook pueda usarlo.
pub fn set_reporter(handle: ReporterHandle) {
    if REPORTER.set(handle).is_err() {
        tracing::warn!("reporter: handler already registered — ignoring duplicate");
    }
}

/// Instala el panic hook personalizado que envía panics al reporter.
/// Debe llamarse al inicio de `main()`.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Enviar al reporter si está disponible
        if let Some(reporter) = REPORTER.get() {
            let msg = panic_message(info);
            let backtrace = panic_backtrace();
            reporter.send(ReportEvent {
                level: EventLevel::Error,
                message: format!("panic: {msg}\n{backtrace}"),
                timestamp: now_secs(),
            });
            // Dar tiempo al worker para enviar antes de que el proceso muera
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        // Ejecutar el hook por defecto para el mensaje estándar y volcado
        default_hook(info);
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "panic desconocido".to_string()
    }
}

fn panic_backtrace() -> String {
    let bt = std::backtrace::Backtrace::force_capture();
    let s = format!("{bt}");
    crate::diagnostics::sanitize::sanitize_backtrace(&s)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Capa de `tracing` que captura eventos `error` y `warn` del target `baud`
/// y los reenvía al reporter (si está disponible).
///
/// Usa la `OnceLock` global: los eventos se descartan silenciosamente si el
/// reporter aún no se ha registrado.
pub struct ReporterLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ReporterLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let Some(handle) = REPORTER.get() else {
            return;
        };

        let target = event.metadata().target();
        if !target.starts_with("baud") {
            return;
        }

        let level = match *event.metadata().level() {
            tracing::Level::ERROR => EventLevel::Error,
            tracing::Level::WARN => EventLevel::Warn,
            _ => return,
        };

        use tracing::field::Visit;
        struct MessageVisitor(String);
        impl Visit for MessageVisitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "message" {
                    self.0 = value.to_string();
                }
            }
            fn record_error(
                &mut self,
                _field: &tracing::field::Field,
                _value: &(dyn std::error::Error + 'static),
            ) {
            }
        }

        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);

        let message = if visitor.0.is_empty() {
            format!("{}: {}", target, event.metadata().name())
        } else {
            format!("[{target}] {}", visitor.0)
        };

        handle.send(ReportEvent {
            level,
            message,
            timestamp: now_secs(),
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn panic_message_from_str() {
        let result = std::panic::catch_unwind(|| {
            std::panic::panic_any("test panic message");
        });
        match result {
            Err(e) => {
                let msg = e.downcast_ref::<&str>();
                assert_eq!(msg, Some(&"test panic message"));
            }
            Ok(_) => panic!("expected panic"),
        }
    }
}
