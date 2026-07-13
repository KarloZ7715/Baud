//! Gestión del ID de instalación para reportes de error.
//!
//! El ID se genera la primera vez y se persiste en el directorio de datos.
//! Si el usuario nunca acepta el reporte, no se crea el archivo.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Genera un identificador hexadecimal de 32 caracteres, útil como ID de
/// instalación o de evento. Combina timestamp, PID y un contador estático
/// para garantizar unicidad dentro del proceso.
pub fn generate_install_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let pid = std::process::id() as u64;
    let mixed = now
        .wrapping_mul(6364136223846793005)
        .wrapping_add(pid)
        .wrapping_add(counter);

    format!("{mixed:016x}{counter:016x}")
}

/// Carga el ID de instalación persistido, o lo genera y persiste si no existe.
pub fn load_or_create_install_id() -> String {
    let path = install_id_path();
    if let Ok(id) = fs::read_to_string(&path) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let id = generate_install_id();
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
