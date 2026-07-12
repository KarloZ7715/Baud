//! Capacidades anunciadas por el backend de clipboard.

/// Capacidades reales del backend activo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub clipboard: bool,
    pub primary: bool,
}

impl Capabilities {
    pub const NONE: Self = Self {
        clipboard: false,
        primary: false,
    };

    pub const CLIPBOARD_ONLY: Self = Self {
        clipboard: true,
        primary: false,
    };

    pub const FULL: Self = Self {
        clipboard: true,
        primary: true,
    };
}

/// Si se pide primary y el backend no lo soporta, usa clipboard.
pub fn resolve_primary(want_primary: bool, caps: Capabilities) -> bool {
    want_primary && caps.primary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_primary_con_caps_remapea_a_clipboard() {
        assert!(!resolve_primary(true, Capabilities::CLIPBOARD_ONLY));
        assert!(!resolve_primary(false, Capabilities::CLIPBOARD_ONLY));
    }

    #[test]
    fn resolve_primary_con_primary_honra_pedido() {
        assert!(resolve_primary(true, Capabilities::FULL));
        assert!(!resolve_primary(false, Capabilities::FULL));
    }
}
