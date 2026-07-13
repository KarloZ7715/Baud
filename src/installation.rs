//! Deteccion de instalaciones gestionadas por el instalador oficial de Baud.
//!
//! El updater solo puede actuar cuando el binario en ejecucion tiene un
//! recibo oficial co-ubicado y canonicalizado. Cualquier otro caso se
//! rechaza antes de realizar peticiones de red o mutaciones.

use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;

/// Instalacion oficial reconocida por recibo.
#[derive(Debug, Clone)]
pub struct Installation {
    pub binary_path: PathBuf,
    pub data_dir: PathBuf,
}

/// Errores de propiedad/alcance que impiden una actualizacion.
#[derive(Debug)]
pub enum OwnershipError {
    /// Instalacion no oficial: instruccion generica.
    NotOwned,
    /// Instalacion oficial anterior sin recibo: reinstalar una vez.
    LegacyLocation,
    /// Instalacion root sin privilegios: instruccion con sudo.
    RootNeedsSudo { path: PathBuf },
}

impl OwnershipError {
    pub fn write_to(&self, writer: &mut dyn Write) -> io::Result<()> {
        match self {
            OwnershipError::NotOwned => writeln!(
                writer,
                "Error: this Baud installation is not managed by the official installer. Update it using the method that installed it."
            ),
            OwnershipError::LegacyLocation => writeln!(
                writer,
                "Error: this installation predates the ownership receipt. Run the official installer once to enable `baud update`."
            ),
            OwnershipError::RootNeedsSudo { path } => writeln!(
                writer,
                "Error: this installation is owned by root. Run: sudo {} update",
                path.display()
            ),
        }
    }
}

impl fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = Vec::new();
        self.write_to(&mut buf).map_err(|_| fmt::Error)?;
        write!(f, "{}", String::from_utf8_lossy(&buf).trim_end())
    }
}

/// Resuelve la instalacion a partir del ejecutable en curso.
pub fn resolve() -> Result<Installation, OwnershipError> {
    // Relleno temporal: se implementa en U2.
    let exe = std::env::current_exe().map_err(|_| OwnershipError::NotOwned)?;
    let parent = exe.parent().ok_or(OwnershipError::NotOwned)?;
    let receipt = parent.join(".baud-install.toml");
    if receipt.exists() {
        Ok(Installation {
            binary_path: exe.clone(),
            data_dir: parent.join("share"),
        })
    } else {
        Err(OwnershipError::NotOwned)
    }
}
