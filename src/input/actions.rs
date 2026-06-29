use crate::input::keymap::{Key, Mods};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Copy,
    Paste,
    PastePrimary,
    ToggleCopyMode,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    FontZoomIn,
    FontZoomOut,
    FontZoomReset,
}

/// Mapa de combinaciones de tecla a acciones del terminal.
#[derive(Debug, Clone)]
pub struct Keybindings {
    bindings: Vec<(Key, Mods, Action)>,
}

impl Keybindings {
    pub fn lookup(&self, key: Key, mods: Mods) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(k, m, _)| *k == key && *m == mods)
            .map(|(_, _, a)| *a)
    }

    /// Inserta o reemplaza un binding (usado por overrides de config).
    pub fn set(&mut self, key: Key, mods: Mods, action: Action) {
        self.bindings.retain(|(k, m, _)| !(*k == key && *m == mods));
        self.bindings.push((key, mods, action));
    }
}

impl Default for Keybindings {
    fn default() -> Self {
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        let alt = Mods {
            alt: true,
            ..Mods::NONE
        };
        Self {
            bindings: vec![
                (Key::Char('c'), cs, Action::Copy),
                (Key::Char('v'), cs, Action::Paste),
                (Key::Char('x'), cs, Action::ToggleCopyMode),
                (Key::Char('='), ctrl, Action::FontZoomIn),
                (Key::Char('-'), ctrl, Action::FontZoomOut),
                (Key::Char('0'), ctrl, Action::FontZoomReset),
                (Key::Up, cs, Action::ScrollLineUp),
                (Key::Down, cs, Action::ScrollLineDown),
                (Key::Up, alt, Action::ScrollPageUp),
                (Key::Down, alt, Action::ScrollPageDown),
                (Key::PageUp, shift, Action::ScrollPageUp),
                (Key::PageDown, shift, Action::ScrollPageDown),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_bindings_copy_paste() {
        let kb = Keybindings::default();
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('c'), cs), Some(Action::Copy));
        assert_eq!(kb.lookup(Key::Char('v'), cs), Some(Action::Paste));
        assert_eq!(kb.lookup(Key::Char('x'), cs), Some(Action::ToggleCopyMode));
    }

    #[test]
    fn test_default_bindings_font_zoom() {
        let kb = Keybindings::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('='), ctrl), Some(Action::FontZoomIn));
        assert_eq!(kb.lookup(Key::Char('-'), ctrl), Some(Action::FontZoomOut));
        assert_eq!(kb.lookup(Key::Char('0'), ctrl), Some(Action::FontZoomReset));
    }

    #[test]
    fn test_lookup_sin_binding_es_none() {
        let kb = Keybindings::default();
        assert_eq!(kb.lookup(Key::Char('a'), Mods::NONE), None);
    }
}
