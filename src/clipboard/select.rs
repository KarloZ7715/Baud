//! Selección pura del tipo de backend según el entorno.

/// Tipo de backend seleccionado a partir del entorno (sin compositor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Wayland,
    X11,
    Windows,
    Null,
}

/// Instantánea de variables de entorno relevantes para la selección.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvSnapshot {
    pub wayland_display: bool,
    pub display: bool,
    pub windows: bool,
}

impl EnvSnapshot {
    /// Lee el entorno del proceso actual.
    pub fn from_process() -> Self {
        Self {
            wayland_display: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            display: std::env::var_os("DISPLAY").is_some(),
            windows: cfg!(windows),
        }
    }
}

/// Prefiere Wayland si hay `WAYLAND_DISPLAY`; si no, X11/`DISPLAY`; si no, Windows; si no, Null.
pub fn select_backend_kind(env: &EnvSnapshot) -> BackendKind {
    if env.wayland_display {
        BackendKind::Wayland
    } else if env.display {
        BackendKind::X11
    } else if env.windows {
        BackendKind::Windows
    } else {
        BackendKind::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matriz_selector_wayland_x11_windows_null() {
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: true,
                display: true,
                windows: false,
            }),
            BackendKind::Wayland
        );
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: false,
                display: true,
                windows: false,
            }),
            BackendKind::X11
        );
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: false,
                display: false,
                windows: true,
            }),
            BackendKind::Windows
        );
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: false,
                display: false,
                windows: false,
            }),
            BackendKind::Null
        );
    }
}
