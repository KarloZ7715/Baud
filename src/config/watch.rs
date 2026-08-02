//! Detección de cambios del archivo de config y del tema importado, por mtime
//! (poll simple).

use std::path::PathBuf;
use std::time::SystemTime;

/// Ruta adicional a vigilar junto al config, y cómo obtener su mtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchTarget {
    /// Archivo normal: se seguiría un enlace simbólico hasta el destino.
    File(PathBuf),
    /// Enlace: se vigila su propio mtime sin seguirlo, para notar cuándo se
    /// recrea apuntando a otro destino (p. ej. `omarchy-theme-set`).
    Link(PathBuf),
}

fn watch_target_mtime(target: &WatchTarget) -> Option<SystemTime> {
    match target {
        WatchTarget::File(p) => std::fs::metadata(p).ok()?.modified().ok(),
        WatchTarget::Link(p) => std::fs::symlink_metadata(p).ok()?.modified().ok(),
    }
}

pub struct WatchState {
    last: Vec<Option<SystemTime>>,
    /// Rutas extra del tema importado activo (archivo, enlace de Omarchy…).
    import_targets: Vec<WatchTarget>,
}

impl WatchState {
    pub fn new(initial_config_mtime: Option<SystemTime>) -> Self {
        Self {
            last: vec![initial_config_mtime],
            import_targets: Vec::new(),
        }
    }

    /// Rutas extra a vigilar además del config (se recalculan tras cada
    /// resolución de tema: carga, recarga, o cambio de esquema del SO).
    pub fn set_import_targets(&mut self, targets: Vec<WatchTarget>) {
        self.import_targets = targets;
    }

    fn signature(&self, config_mtime: Option<SystemTime>) -> Vec<Option<SystemTime>> {
        let mut sig = vec![config_mtime];
        sig.extend(self.import_targets.iter().map(watch_target_mtime));
        sig
    }

    /// `true` si el config o cualquier ruta de import difiere de lo último visto.
    pub fn changed(&mut self, config_mtime: Option<SystemTime>) -> bool {
        let current = self.signature(config_mtime);
        if current != self.last {
            self.last = current;
            true
        } else {
            false
        }
    }

    /// Fija el estado conocido sin disparar recarga (p. ej. tras escribir desde el picker).
    pub fn sync(&mut self, config_mtime: Option<SystemTime>) {
        self.last = self.signature(config_mtime);
    }
}

/// mtime del primer archivo de config existente (mismo orden que [`super::Config::load`]).
pub fn config_mtime() -> Option<SystemTime> {
    let paths = [
        dirs::config_dir()
            .map(|d| d.join("baud").join("config.toml"))
            .unwrap_or_default(),
        std::path::PathBuf::from("baud.toml"),
    ];
    for path in &paths {
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(mtime) = meta.modified() {
                return Some(mtime);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn detecta_cambio_de_mtime() {
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(1);
        let mut state = WatchState::new(Some(t0));
        assert!(!state.changed(Some(t0)));
        assert!(state.changed(Some(t1)));
        assert!(!state.changed(Some(t1)));
        assert!(state.changed(None));
    }

    #[test]
    fn cambio_en_archivo_importado_dispara_changed_aunque_el_config_no_cambie() {
        let path =
            std::env::temp_dir().join(format!("baud_watch_import_{}.ini", std::process::id()));
        std::fs::write(&path, "a").unwrap();
        let config_mtime = Some(SystemTime::UNIX_EPOCH);
        let mut state = WatchState::new(config_mtime);
        state.set_import_targets(vec![WatchTarget::File(path.clone())]);
        // Sincronizar contra el mtime actual del archivo antes de comparar.
        state.sync(config_mtime);
        assert!(!state.changed(config_mtime));

        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&path, "b").unwrap();
        assert!(state.changed(config_mtime));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sync_evita_recarga_por_una_escritura_propia() {
        let path = std::env::temp_dir().join(format!("baud_watch_sync_{}.ini", std::process::id()));
        std::fs::write(&path, "a").unwrap();
        let mut state = WatchState::new(None);
        state.set_import_targets(vec![WatchTarget::File(path.clone())]);
        state.sync(None);
        assert!(!state.changed(None));
        let _ = std::fs::remove_file(&path);
    }
}
