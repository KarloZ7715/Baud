//! Actualizador automatico verificado para instalaciones oficiales de Baud.
//!
//! Descubre la ultima release, verifica un manifiesto firmado y el digest del
//! asset, y reemplaza el binario y los recursos del launcher de forma atomica
//! solo cuando todo ha sido validado.

use std::fmt;

use crate::installation::Installation;

/// Actualizador para una instalacion oficial reconocida.
pub struct Updater {
    installation: Installation,
}

impl Updater {
    pub fn new(installation: Installation) -> Self {
        Self { installation }
    }

    pub fn run(&self) -> Result<(), UpdateError> {
        // Relleno temporal: se implementa en U3.
        let _ = &self.installation.binary_path;
        Err(UpdateError::Internal("updater not yet implemented".into()))
    }
}

/// Errores que pueden ocurrir durante una actualizacion.
#[derive(Debug)]
pub enum UpdateError {
    Internal(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}
