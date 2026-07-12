//! Gestión del ID de instalación para reportes de error.
//!
//! El ID se genera la primera vez y se persiste en el directorio de datos.
//! Si el usuario nunca acepta el reporte, no se crea el archivo.

use std::fs;
use std::path::PathBuf;

/// Carga el ID de instalación, o lo genera y persiste si no existe.
pub fn load_or_create_install_id() -> String {
    let path = install_id_path();
    if let Ok(id) = fs::read_to_string(&path) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let id = crate::diagnostics::reporter::generate_install_id();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, &id);
    id
}

fn install_id_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("baud")
        .join("install_id")
}
