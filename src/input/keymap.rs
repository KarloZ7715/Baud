//! Codificacion pura de teclas a bytes para el PTY.
//! No depende de winit: window.rs traduce sus eventos a estos tipos.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub sup: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        shift: false,
        alt: false,
        ctrl: false,
        sup: false,
    };
    pub fn any(&self) -> bool {
        self.shift || self.alt || self.ctrl || self.sup
    }

    /// Parametro de modificador estilo xterm: 1 + bitmask
    /// (shift=1, alt=2, ctrl=4, super=8).
    pub fn xterm_param(&self) -> u8 {
        let mut m = 0;
        if self.shift {
            m += 1;
        }
        if self.alt {
            m += 2;
        }
        if self.ctrl {
            m += 4;
        }
        if self.sup {
            m += 8;
        }
        1 + m
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// Teclas de funcion F1..F12.
    F(u8),
}

/// Modos del terminal que afectan el encoding (vienen de Term).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModes {
    /// DECCKM: application cursor keys (flechas/Home/End en SS3).
    pub app_cursor_keys: bool,
    /// DECKPAM: application keypad.
    pub app_keypad: bool,
    /// LNM: newline mode (Enter envia CR+LF).
    pub newline_mode: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mods_xterm_param() {
        assert_eq!(Mods::NONE.xterm_param(), 1);
        assert_eq!(
            Mods {
                shift: true,
                ..Mods::NONE
            }
            .xterm_param(),
            2
        );
        assert_eq!(
            Mods {
                alt: true,
                ..Mods::NONE
            }
            .xterm_param(),
            3
        );
        assert_eq!(
            Mods {
                ctrl: true,
                ..Mods::NONE
            }
            .xterm_param(),
            5
        );
        assert_eq!(
            Mods {
                ctrl: true,
                shift: true,
                ..Mods::NONE
            }
            .xterm_param(),
            6
        );
    }
}
