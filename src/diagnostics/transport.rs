//! Transporte HTTP para el envío de envelopes a Sentry.
//!
//! Proporciona una abstracción `Transport` con dos implementaciones:
//! - `UreqTransport`: envía vía HTTPS (TLS verificado).
//! - `MockTransport`: para tests, registra los envíos en un `Vec`.

use std::sync::Mutex;

/// Abstracción de transporte para enviar un envelope a Sentry.
/// Fail-closed: si falla, la app sigue sin error fatal.
pub trait Transport: Send + Sync {
    /// Envía el envelope (cuerpo HTTP ya construido).
    /// Devuelve `Ok(())` si se envió con éxito o `Err` con descripción del fallo.
    fn send(&self, dsn: &str, envelope: &str) -> Result<(), String>;
}

/// Transporte real que envía envelopes vía HTTPS a Sentry.
#[derive(Default)]
pub struct UreqTransport;

impl UreqTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Transport for UreqTransport {
    fn send(&self, dsn: &str, envelope: &str) -> Result<(), String> {
        let project_id = extract_project_id(dsn)?;
        let url = format!(
            "https://{}.ingest.{}.sentry.io/api/{}/envelope/",
            extract_host_prefix(dsn)?,
            extract_region(dsn).unwrap_or("us"),
            project_id
        );

        let agent = ureq::agent();

        let response = agent
            .post(&url)
            .header("Content-Type", "application/x-sentry-envelope")
            .header(
                "X-Sentry-Auth",
                &format!(
                    "Sentry sentry_version=7,sentry_client=baud/{},sentry_key={}",
                    env!("CARGO_PKG_VERSION"),
                    extract_public_key(dsn).unwrap_or("unknown")
                ),
            )
            .send(envelope)
            .map_err(|e| format!("error de red: {e}"))?;

        let status = response.status();
        if status == 200 || status == 202 {
            Ok(())
        } else {
            let body = response.into_body().read_to_string().unwrap_or_default();
            Err(format!("HTTP {status}: {body}"))
        }
    }
}

/// Extrae el ID del proyecto del DSN.
/// Formato DSN: `https://<key>@<host_prefix>.ingest.<region>.sentry.io/<project_id>`
fn extract_project_id(dsn: &str) -> Result<&str, String> {
    let parts: Vec<&str> = dsn.rsplitn(2, ".sentry.io/").collect();
    if parts.len() < 2 {
        return Err("DSN sin .sentry.io/".to_string());
    }
    let id = parts[0].split('?').next().unwrap_or(parts[0]);
    if id.is_empty() {
        return Err("project_id vacío en DSN".to_string());
    }
    Ok(id)
}

fn extract_host_prefix(dsn: &str) -> Result<&str, String> {
    let dsn = dsn.strip_prefix("https://").unwrap_or(dsn);
    let prefix = dsn
        .split('@')
        .nth(1)
        .and_then(|s| s.split(".ingest.").next())
        .ok_or_else(|| "DSN sin host prefix".to_string())?;
    Ok(prefix)
}

fn extract_region(dsn: &str) -> Option<&str> {
    dsn.split(".ingest.").nth(1)?.split(".sentry.io").next()
}

fn extract_public_key(dsn: &str) -> Option<&str> {
    dsn.strip_prefix("https://")?
        .split('@')
        .next()
        .map(|s| s.rsplit(':').next_back().unwrap_or(s))
}

/// Transporte mock que registra los envíos en memoria para tests.
#[derive(Default)]
pub struct MockTransport {
    pub sent: Mutex<Vec<String>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
        }
    }

    pub fn reset(&self) {
        self.sent.lock().unwrap().clear();
    }

    pub fn count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

impl Transport for MockTransport {
    fn send(&self, _dsn: &str, envelope: &str) -> Result<(), String> {
        self.sent.lock().unwrap().push(envelope.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_project_id_de_dsn_valido() {
        let dsn = "https://abc123@o0.ingest.us.sentry.io/456789";
        assert_eq!(extract_project_id(dsn).unwrap(), "456789");
    }

    #[test]
    fn extract_project_id_devuelve_error_con_dsn_invalido() {
        assert!(extract_project_id("no-es-un-dsn").is_err());
    }

    #[test]
    fn mock_transport_registra_envios() {
        let transport = MockTransport::new();
        assert!(transport
            .send("https://k@o0.ingest.us.sentry.io/1", "test")
            .is_ok());
        assert_eq!(transport.count(), 1);
    }

    #[test]
    fn mock_transport_reset_limpia() {
        let transport = MockTransport::new();
        transport
            .send("https://k@o0.ingest.us.sentry.io/1", "e1")
            .unwrap();
        transport.reset();
        assert_eq!(transport.count(), 0);
    }
}
