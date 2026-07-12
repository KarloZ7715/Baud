//! Consentimiento del usuario para reporte remoto de errores.
//!
//! El estado de consentimiento se deriva de `diagnostics.reporting.enabled`
//! en la config. La primera vez que el usuario elige Sí o No en el modal,
//! se persiste en config.toml mediante `persist_reporting_enabled`.

use std::fs;
use std::path::PathBuf;

use toml_edit::{DocumentMut, Item, Table, Value};

/// Estado de consentimiento del usuario para el reporte remoto de errores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentState {
    /// No ha decidido de forma definitiva. Se debe mostrar el modal.
    Unset,
    /// Aceptó enviar informes de error a Sentry.
    Accepted,
    /// Rechazó enviar informes.
    Declined,
}

impl ConsentState {
    /// Deriva el estado desde el valor `enabled` de la config.
    /// `None` = nunca decidió; `Some(true)` = aceptó; `Some(false)` = rechazó.
    pub fn from_config(enabled: Option<bool>) -> Self {
        match enabled {
            Some(true) => Self::Accepted,
            Some(false) => Self::Declined,
            None => Self::Unset,
        }
    }

    /// `true` si el usuario ya tomó una decisión (Accepted o Declined).
    pub fn is_decided(self) -> bool {
        matches!(self, Self::Accepted | Self::Declined)
    }
}

/// Escribe `[diagnostics.reporting] enabled = bool` en config.toml.
/// Crea el archivo y los directorios padres si no existen.
/// Devuelve la ruta del archivo escrito.
pub fn persist_reporting_enabled(enabled: bool) -> Result<PathBuf, PersistError> {
    let path = crate::config::persist::config_write_path();

    if path.exists() {
        let content = fs::read_to_string(&path).map_err(|e| PersistError::Io(e.to_string()))?;
        let mut doc = content
            .parse::<DocumentMut>()
            .map_err(|e| PersistError::Parse(e.to_string()))?;
        ensure_diagnostics_section(&mut doc, enabled);
        fs::write(&path, doc.to_string()).map_err(|e| PersistError::Io(e.to_string()))?;
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| PersistError::Io(e.to_string()))?;
        }
        let mut doc = DocumentMut::new();
        ensure_diagnostics_section(&mut doc, enabled);
        fs::write(&path, doc.to_string()).map_err(|e| PersistError::Io(e.to_string()))?;
    }

    Ok(path)
}

/// Asegura que el documento TOML tenga `[diagnostics.reporting]` con `enabled`.
fn ensure_diagnostics_section(doc: &mut DocumentMut, enabled: bool) {
    let diag = doc
        .entry("diagnostics")
        .or_insert_with(|| Item::Table(Table::new()));

    if let Item::Table(diag_table) = diag {
        let reporting = diag_table
            .entry("reporting")
            .or_insert_with(|| Item::Table(Table::new()));

        if let Item::Table(reporting_table) = reporting {
            reporting_table.insert("enabled", Item::Value(Value::from(enabled)));
        }
    }
}

/// Error al persistir el consentimiento.
#[derive(Debug)]
pub enum PersistError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "I/O: {msg}"),
            Self::Parse(msg) => write!(f, "parse: {msg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("baud_consent_{}_{label}.toml", std::process::id()))
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn consent_from_config_unset() {
        assert_eq!(ConsentState::from_config(None), ConsentState::Unset);
    }

    #[test]
    fn consent_from_config_accepted() {
        assert_eq!(
            ConsentState::from_config(Some(true)),
            ConsentState::Accepted
        );
    }

    #[test]
    fn consent_from_config_declined() {
        assert_eq!(
            ConsentState::from_config(Some(false)),
            ConsentState::Declined
        );
    }

    #[test]
    fn consent_is_decided() {
        assert!(!ConsentState::Unset.is_decided());
        assert!(ConsentState::Accepted.is_decided());
        assert!(ConsentState::Declined.is_decided());
    }

    #[test]
    fn persist_enabled_true_crea_archivo() {
        let path = temp_config_path("new_true");
        cleanup(&path);
        // Forzamos la ruta para el test; persist_reporting_enabled usa config_write_path
        // que depende de dirs::config_dir(). Para tests usamos una ruta fija.
        persist_at(&path, true).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[diagnostics]"));
        assert!(content.contains("[diagnostics.reporting]"));
        assert!(content.contains("enabled = true"));
        cleanup(&path);
    }

    #[test]
    fn persist_enabled_false_crea_archivo() {
        let path = temp_config_path("new_false");
        cleanup(&path);
        persist_at(&path, false).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("enabled = false"));
        cleanup(&path);
    }

    #[test]
    fn persist_sobre_archivo_existente_preserva_otras_claves() {
        let path = temp_config_path("existing");
        cleanup(&path);
        fs::write(&path, "theme = \"nord\"\nfont.size = 14\n").unwrap();
        persist_at(&path, true).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("theme = \"nord\""));
        assert!(content.contains("font.size = 14"));
        assert!(content.contains("enabled = true"));
        cleanup(&path);
    }

    #[test]
    fn persist_sobrescribe_enabled_existente() {
        let path = temp_config_path("overwrite");
        cleanup(&path);
        fs::write(&path, "[diagnostics.reporting]\nenabled = true\n").unwrap();
        persist_at(&path, false).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("enabled = false"));
        assert!(!content.contains("enabled = true"));
        cleanup(&path);
    }

    /// Versión de persist que acepta una ruta explícita (para tests).
    fn persist_at(path: &PathBuf, enabled: bool) -> Result<(), PersistError> {
        if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| PersistError::Io(e.to_string()))?;
            let mut doc = content
                .parse::<DocumentMut>()
                .map_err(|e| PersistError::Parse(e.to_string()))?;
            ensure_diagnostics_section(&mut doc, enabled);
            fs::write(path, doc.to_string()).map_err(|e| PersistError::Io(e.to_string()))?;
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| PersistError::Io(e.to_string()))?;
            }
            let mut doc = DocumentMut::new();
            ensure_diagnostics_section(&mut doc, enabled);
            fs::write(path, doc.to_string()).map_err(|e| PersistError::Io(e.to_string()))?;
        }
        Ok(())
    }
}
