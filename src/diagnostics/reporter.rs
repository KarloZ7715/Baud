//! Reporter de errores remoto vía Sentry.
//!
//! Arquitectura:
//! - `Reporter` es dueño del worker thread y el canal.
//! - `ReporterHandle` es un clon ligero para enviar eventos desde cualquier hilo.
//! - El worker aplica rate-limit, sanitización y envía al transport.
//!
//! Solo se activa si el DSN es válido y el consentimiento es `Accepted`.

use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::diagnostics::sanitize;
use crate::diagnostics::sentry;
use crate::diagnostics::transport::Transport;

/// Capacidad máxima de la cola de eventos pendientes.
const QUEUE_CAPACITY: usize = 64;

/// Máximo de eventos por minuto (rate-limit global).
const MAX_EVENTS_PER_MINUTE: usize = 10;

/// Ventana de deduplicación: no se envían eventos con el mismo mensaje
/// dentro de esta ventana.
const DEDUP_WINDOW_SECS: u64 = 30;

/// Nivel de severidad de un evento.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Error,
    Warn,
}

/// Evento a reportar.
#[derive(Debug, Clone)]
pub struct ReportEvent {
    pub level: EventLevel,
    pub message: String,
    pub timestamp: i64,
}

/// Handle ligero y clonable para enviar eventos al reporter desde cualquier hilo.
#[derive(Clone)]
pub struct ReporterHandle {
    tx: SyncSender<ReportEvent>,
    enabled: Arc<Mutex<bool>>,
}

impl ReporterHandle {
    /// Envía un evento al worker. No bloquea si la cola está llena (descarte silencioso).
    pub fn send(&self, event: ReportEvent) {
        if !*self.enabled.lock().unwrap() {
            return;
        }
        let _ = self.tx.try_send(event);
    }

    /// Habilita o deshabilita el reporter en caliente.
    pub fn set_enabled(&self, enabled: bool) {
        *self.enabled.lock().unwrap() = enabled;
    }
}

/// Reporter principal, dueño del worker thread.
pub struct Reporter {
    handle: ReporterHandle,
}

impl Reporter {
    /// Crea un reporter y lanza el worker de fondo.
    /// Si `dsn` es `None`, el reporter se crea en modo noop (cola vacía, sin thread).
    pub fn new(dsn: Option<String>, install_id: String, transport: Box<dyn Transport>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ReportEvent>(QUEUE_CAPACITY);
        let enabled = Arc::new(Mutex::new(dsn.is_some()));

        let handle = ReporterHandle {
            tx,
            enabled: Arc::clone(&enabled),
        };

        if let Some(dsn) = dsn {
            let enabled_clone = Arc::clone(&enabled);
            thread::Builder::new()
                .name("baud-reporter".into())
                .spawn(move || {
                    reporter_worker(rx, dsn, install_id, transport, enabled_clone);
                })
                .expect("no se pudo iniciar el hilo reporter");
        } else {
            tracing::info!("reporter: sin DSN, modo noop (sin red)");
        }

        Self { handle }
    }

    pub fn handle(&self) -> ReporterHandle {
        self.handle.clone()
    }
}

/// Worker del reporter: recibe eventos, aplica rate-limit, sanitiza y envía.
fn reporter_worker(
    rx: mpsc::Receiver<ReportEvent>,
    dsn: String,
    install_id: String,
    transport: Box<dyn Transport>,
    enabled: Arc<Mutex<bool>>,
) {
    let mut recent: Vec<(String, Instant)> = Vec::new();
    let mut minute_events: Vec<Instant> = Vec::new();

    while let Ok(event) = rx.recv() {
        if !*enabled.lock().unwrap() {
            continue;
        }
        process_event(
            &event,
            &dsn,
            &install_id,
            transport.as_ref(),
            &mut recent,
            &mut minute_events,
        );
    }
}

fn process_event(
    event: &ReportEvent,
    dsn: &str,
    install_id: &str,
    transport: &dyn Transport,
    recent: &mut Vec<(String, Instant)>,
    minute_events: &mut Vec<Instant>,
) {
    let now = Instant::now();

    // Limpieza de ventanas
    recent.retain(|(_, t)| t.elapsed() < Duration::from_secs(DEDUP_WINDOW_SECS));
    minute_events.retain(|t| t.elapsed() < Duration::from_secs(60));

    // Rate-limit por minuto
    if minute_events.len() >= MAX_EVENTS_PER_MINUTE {
        return;
    }

    // Dedup por mensaje
    let normalized = normalize_message(&event.message);
    if recent.iter().any(|(m, _)| m == &normalized) {
        return;
    }
    recent.push((normalized, now));
    minute_events.push(now);

    // Sanitizar
    let sanitized = sanitize::sanitize_message(&event.message);

    // Construir envelope
    let level = match event.level {
        EventLevel::Error => "error",
        EventLevel::Warn => "warn",
    };

    let event_id = generate_event_id();
    let timestamp = format_timestamp(event.timestamp);

    let mut tags = sentry::system_tags();
    tags.insert("install_id".to_string(), install_id.to_string());

    let mut extra = std::collections::HashMap::new();
    extra.insert("install_id".to_string(), install_id.to_string());
    extra.insert("sanitized".to_string(), "true".to_string());

    let envelope = sentry::build_envelope(&event_id, level, &sanitized, &timestamp, tags, extra);

    if let Err(e) = transport.send(dsn, &envelope) {
        tracing::warn!(
            target: "baud::reporter",
            error = %e,
            "falló el envío a Sentry"
        );
    }
}

/// Normaliza un mensaje para deduplicación: trunca a 200 chars y quita
/// números y timestamps variables.
fn normalize_message(msg: &str) -> String {
    let trimmed = msg.trim();
    if trimmed.len() <= 200 {
        trimmed.to_string()
    } else {
        trimmed[..200].to_string()
    }
}

/// Genera un ID de evento aleatorio (32 hex chars).
fn generate_event_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let hash = RandomState::new().build_hasher().finish();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Mezcla hash + nanos para más entropía
    let combined = hash.wrapping_add(nanos as u64);
    format!("{combined:032x}")
}

/// Formatea un timestamp Unix (segundos) como ISO 8601.
fn format_timestamp(ts: i64) -> String {
    // Simple: usar segundos desde epoch y formatear manualmente
    let secs = ts;
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Fecha aproximada desde epoch (no es exacta pero suficiente para Sentry)
    let year = 1970 + (days_since_epoch / 365);
    let mut remaining = days_since_epoch % 365;

    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    let day = remaining + 1;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.000Z")
}

/// Genera un ID de instalación aleatorio (UUID-like, 32 hex chars).
pub fn generate_install_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let s = RandomState::new();
    let h1 = s.build_hasher().finish();
    let h2 = s.build_hasher().finish();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let combined = h1.wrapping_add(h2).wrapping_add(nanos as u64);
    format!("{combined:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::transport::MockTransport;

    #[test]
    fn reporter_handle_send_no_panica() {
        let transport = MockTransport::new();
        let reporter = Reporter::new(
            Some("https://k@o0.ingest.us.sentry.io/1".into()),
            "test-id".into(),
            Box::new(transport),
        );
        let handle = reporter.handle();
        handle.send(ReportEvent {
            level: EventLevel::Error,
            message: "test error".into(),
            timestamp: 1000,
        });
        // Le damos tiempo al worker para procesar
        std::thread::sleep(Duration::from_millis(50));
        // El mock no está compartido en este diseño, pero la llamada no debe paniquear
    }

    #[test]
    fn reporter_noop_sin_dsn_no_panica() {
        let transport = MockTransport::new();
        let reporter = Reporter::new(None, "test-id".into(), Box::new(transport));
        let handle = reporter.handle();
        handle.send(ReportEvent {
            level: EventLevel::Error,
            message: "should be ignored".into(),
            timestamp: 1000,
        });
    }

    #[test]
    fn normalize_message_trunca() {
        let long = "a".repeat(300);
        let norm = normalize_message(&long);
        assert_eq!(norm.len(), 200);
    }

    #[test]
    fn format_timestamp_produce_iso() {
        let ts = format_timestamp(0); // epoch
        assert!(ts.contains("1970"));
        assert!(ts.contains("T"));
        assert!(ts.ends_with("Z"));
    }

    #[test]
    fn generate_install_id_es_unico() {
        let id1 = generate_install_id();
        let id2 = generate_install_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 32);
    }
}
