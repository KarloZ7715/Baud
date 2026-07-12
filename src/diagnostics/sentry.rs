//! Formato de envelope de Sentry.
//!
//! Construye un envelope HTTP compatible con Sentry SaaS / GlitchTip.
//! El envelope se envía como POST a `/api/<project_id>/envelope/` con
//! Content-Type `application/x-sentry-envelope`.

use std::collections::HashMap;

/// Construye el cuerpo completo del envelope de Sentry.
///
/// Formato (líneas separadas por `\n`):
/// 1. Header JSON: `{event_id, sdk, sent_at}`
/// 2. Item header JSON: `{type: "event", content_type: "application/json"}`
/// 3. Event payload JSON
pub fn build_envelope(
    event_id: &str,
    level: &str,
    message: &str,
    timestamp: &str,
    tags: HashMap<String, String>,
    extra: HashMap<String, String>,
) -> String {
    let sent_at = timestamp;
    let header = serde_json::json!({
        "event_id": event_id,
        "sdk": {
            "name": "baud-reporter",
            "version": env!("CARGO_PKG_VERSION")
        },
        "sent_at": sent_at,
    });

    let item_header = serde_json::json!({
        "type": "event",
        "content_type": "application/json",
    });

    let payload = build_event_payload(event_id, level, message, timestamp, tags, extra);

    format!(
        "{}\n{}\n{}\n",
        serde_json::to_string(&header).unwrap_or_default(),
        serde_json::to_string(&item_header).unwrap_or_default(),
        serde_json::to_string(&payload).unwrap_or_default(),
    )
}

fn build_event_payload(
    event_id: &str,
    level: &str,
    message: &str,
    timestamp: &str,
    tags: HashMap<String, String>,
    extra: HashMap<String, String>,
) -> serde_json::Value {
    let level = match level {
        "warn" => "warning",
        other => other,
    };

    serde_json::json!({
        "event_id": event_id,
        "level": level,
        "logger": "baud",
        "platform": "native",
        "timestamp": timestamp,
        "message": {
            "formatted": message,
        },
        "tags": tags,
        "extra": extra,
        "release": option_env!("BAUD_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
    })
}

/// Etiquetas de metadata del sistema (OS, arch).
pub fn system_tags() -> HashMap<String, String> {
    let mut tags = HashMap::new();
    tags.insert("os".to_string(), std::env::consts::OS.to_string());
    tags.insert("arch".to_string(), std::env::consts::ARCH.to_string());
    tags.insert(
        "release".to_string(),
        option_env!("BAUD_RELEASE_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_string(),
    );
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_contiene_lines() {
        let envelope = build_envelope(
            "abc123",
            "error",
            "test message",
            "2026-01-01T00:00:00Z",
            HashMap::new(),
            HashMap::new(),
        );
        let lines: Vec<&str> = envelope.lines().collect();
        assert!(lines.len() >= 3);
    }

    #[test]
    fn envelope_es_json_valido_por_linea() {
        let envelope = build_envelope(
            "abc123",
            "warn",
            "warning test",
            "2026-01-01T00:00:00Z",
            system_tags(),
            HashMap::new(),
        );
        for line in envelope.lines() {
            assert!(
                serde_json::from_str::<serde_json::Value>(line).is_ok(),
                "línea no es JSON válido: {line}"
            );
        }
    }

    #[test]
    fn warn_se_convierte_a_warning() {
        let envelope = build_envelope(
            "abc123",
            "warn",
            "msg",
            "2026-01-01T00:00:00Z",
            HashMap::new(),
            HashMap::new(),
        );
        assert!(envelope.contains("\"level\":\"warning\""));
    }

    #[test]
    fn system_tags_tiene_os_y_arch() {
        let tags = system_tags();
        assert!(tags.contains_key("os"));
        assert!(tags.contains_key("arch"));
    }
}
