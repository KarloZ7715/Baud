//! Detección de cambios del archivo de config por mtime (poll simple).

use std::time::SystemTime;

pub struct WatchState {
    last: Option<SystemTime>,
}

impl WatchState {
    pub fn new(initial: Option<SystemTime>) -> Self {
        Self { last: initial }
    }

    /// `true` si `current` difiere del último visto; actualiza el estado interno.
    pub fn changed(&mut self, current: Option<SystemTime>) -> bool {
        if current != self.last {
            self.last = current;
            true
        } else {
            false
        }
    }

    /// Fija el mtime conocido sin disparar recarga (p. ej. tras escribir desde el picker).
    pub fn sync(&mut self, current: Option<SystemTime>) {
        self.last = current;
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
}
