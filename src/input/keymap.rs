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

/// Codifica una tecla a bytes para el PTY. None = no se envia nada.
pub fn encode_key(key: Key, mods: Mods, modes: KeyModes) -> Option<Vec<u8>> {
    // ponytail: application keypad real requiere distinguir el numpad fisico, que
    // winit no expone de forma fiable. Se deja el encoding numerico por defecto.
    // Upgrade path: mapear PhysicalKey Numpad* en window.rs y pasar un Key::Numpad*.
    match key {
        Key::Char(c) => Some(encode_char(c, mods)),
        Key::Enter => Some(if modes.newline_mode {
            vec![0x0d, 0x0a]
        } else {
            vec![0x0d]
        }),
        Key::Tab => {
            if mods.shift {
                Some(b"\x1b[Z".to_vec())
            } else {
                Some(vec![0x09])
            }
        }
        Key::Backspace => {
            // Ctrl+Backspace -> BS (0x08); Alt antepone ESC.
            let base = if mods.ctrl { 0x08 } else { 0x7f };
            Some(if mods.alt {
                vec![0x1b, base]
            } else {
                vec![base]
            })
        }
        Key::Escape => Some(if mods.alt {
            vec![0x1b, 0x1b]
        } else {
            vec![0x1b]
        }),
        Key::Up | Key::Down | Key::Right | Key::Left | Key::Home | Key::End => {
            encode_cursor(key, mods, modes)
        }
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => encode_tilde(key, mods),
        Key::F(n) => encode_fkey(n, mods),
    }
}

/// Codifica un caracter aplicando Ctrl (& 0x1F sobre ASCII) y prefijo Alt (ESC).
fn encode_char(c: char, mods: Mods) -> Vec<u8> {
    let mut bytes = Vec::new();
    if mods.ctrl && c.is_ascii() {
        // control: letra -> 0x01..0x1a; otros segun mascara ASCII.
        bytes.push((c as u8).to_ascii_uppercase() & 0x1f);
    } else {
        let mut buf = [0u8; 4];
        bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
    if mods.alt {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        out.push(0x1b);
        out.extend_from_slice(&bytes);
        out
    } else {
        bytes
    }
}

/// Letra final para teclas tipo cursor.
fn cursor_letter(key: Key) -> Option<u8> {
    Some(match key {
        Key::Up => b'A',
        Key::Down => b'B',
        Key::Right => b'C',
        Key::Left => b'D',
        Key::Home => b'H',
        Key::End => b'F',
        _ => return None,
    })
}

/// Codifica teclas tipo cursor (flechas/Home/End).
fn encode_cursor(key: Key, mods: Mods, modes: KeyModes) -> Option<Vec<u8>> {
    let letter = cursor_letter(key)?;
    if mods.any() {
        // CSI 1 ; <mod> <letra>
        Some(format!("\x1b[1;{}{}", mods.xterm_param(), letter as char).into_bytes())
    } else if modes.app_cursor_keys {
        // SS3 <letra>
        Some(vec![0x1b, b'O', letter])
    } else {
        // CSI <letra>
        Some(vec![0x1b, b'[', letter])
    }
}

/// Codigo numerico para teclas con terminador '~'.
fn tilde_code(key: Key) -> Option<u16> {
    Some(match key {
        Key::Insert => 2,
        Key::Delete => 3,
        Key::PageUp => 5,
        Key::PageDown => 6,
        _ => return None,
    })
}

fn encode_tilde(key: Key, mods: Mods) -> Option<Vec<u8>> {
    let code = tilde_code(key)?;
    if mods.any() {
        Some(format!("\x1b[{};{}~", code, mods.xterm_param()).into_bytes())
    } else {
        Some(format!("\x1b[{}~", code).into_bytes())
    }
}

fn encode_fkey(n: u8, mods: Mods) -> Option<Vec<u8>> {
    // F1-F4 usan SS3 (sin mods) o CSI 1;m LETRA (con mods).
    let ss3 = match n {
        1 => Some(b'P'),
        2 => Some(b'Q'),
        3 => Some(b'R'),
        4 => Some(b'S'),
        _ => None,
    };
    if let Some(letter) = ss3 {
        return Some(if mods.any() {
            format!("\x1b[1;{}{}", mods.xterm_param(), letter as char).into_bytes()
        } else {
            vec![0x1b, b'O', letter]
        });
    }
    // F5..F12 usan CSI <code> ~
    let code: u16 = match n {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    };
    Some(if mods.any() {
        format!("\x1b[{};{}~", code, mods.xterm_param()).into_bytes()
    } else {
        format!("\x1b[{}~", code).into_bytes()
    })
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

    #[test]
    fn test_encode_char_simple() {
        assert_eq!(
            encode_key(Key::Char('a'), Mods::NONE, KeyModes::default()),
            Some(b"a".to_vec())
        );
    }

    #[test]
    fn test_encode_ctrl_letra() {
        let m = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key(Key::Char('a'), m, KeyModes::default()),
            Some(vec![0x01])
        );
        assert_eq!(
            encode_key(Key::Char('c'), m, KeyModes::default()),
            Some(vec![0x03])
        );
    }

    #[test]
    fn test_encode_alt_letra_prefijo_esc() {
        let m = Mods {
            alt: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key(Key::Char('f'), m, KeyModes::default()),
            Some(vec![0x1b, b'f'])
        );
    }

    #[test]
    fn test_encode_ctrl_alt_letra() {
        let m = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        // Alt+Ctrl+a = ESC + 0x01
        assert_eq!(
            encode_key(Key::Char('a'), m, KeyModes::default()),
            Some(vec![0x1b, 0x01])
        );
    }

    #[test]
    fn test_encode_enter_backspace_tab() {
        assert_eq!(
            encode_key(Key::Enter, Mods::NONE, KeyModes::default()),
            Some(vec![0x0d])
        );
        assert_eq!(
            encode_key(Key::Backspace, Mods::NONE, KeyModes::default()),
            Some(vec![0x7f])
        );
        assert_eq!(
            encode_key(Key::Tab, Mods::NONE, KeyModes::default()),
            Some(vec![0x09])
        );
        // Shift+Tab = CBT
        let s = Mods {
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key(Key::Tab, s, KeyModes::default()),
            Some(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn test_encode_enter_newline_mode() {
        let modes = KeyModes {
            newline_mode: true,
            ..KeyModes::default()
        };
        assert_eq!(
            encode_key(Key::Enter, Mods::NONE, modes),
            Some(vec![0x0d, 0x0a])
        );
    }

    #[test]
    fn test_arrows_normales() {
        let d = KeyModes::default();
        assert_eq!(encode_key(Key::Up, Mods::NONE, d), Some(b"\x1b[A".to_vec()));
        assert_eq!(
            encode_key(Key::Left, Mods::NONE, d),
            Some(b"\x1b[D".to_vec())
        );
        assert_eq!(
            encode_key(Key::Home, Mods::NONE, d),
            Some(b"\x1b[H".to_vec())
        );
        assert_eq!(
            encode_key(Key::End, Mods::NONE, d),
            Some(b"\x1b[F".to_vec())
        );
    }

    #[test]
    fn test_arrows_app_cursor_keys_ss3() {
        let m = KeyModes {
            app_cursor_keys: true,
            ..KeyModes::default()
        };
        assert_eq!(encode_key(Key::Up, Mods::NONE, m), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            encode_key(Key::Home, Mods::NONE, m),
            Some(b"\x1bOH".to_vec())
        );
    }

    #[test]
    fn test_arrows_con_modificadores() {
        let d = KeyModes::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        // Ctrl+Left = CSI 1;5 D  (incluso con app cursor keys se usa CSI)
        assert_eq!(encode_key(Key::Left, ctrl, d), Some(b"\x1b[1;5D".to_vec()));
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(encode_key(Key::Up, shift, d), Some(b"\x1b[1;2A".to_vec()));
    }

    #[test]
    fn test_tilde_keys() {
        let d = KeyModes::default();
        assert_eq!(
            encode_key(Key::Delete, Mods::NONE, d),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            encode_key(Key::PageUp, Mods::NONE, d),
            Some(b"\x1b[5~".to_vec())
        );
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key(Key::Delete, ctrl, d),
            Some(b"\x1b[3;5~".to_vec())
        );
    }

    #[test]
    fn test_fkeys() {
        let d = KeyModes::default();
        assert_eq!(
            encode_key(Key::F(1), Mods::NONE, d),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            encode_key(Key::F(5), Mods::NONE, d),
            Some(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            encode_key(Key::F(12), Mods::NONE, d),
            Some(b"\x1b[24~".to_vec())
        );
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        // Shift+F1 = CSI 1;2 P
        assert_eq!(encode_key(Key::F(1), shift, d), Some(b"\x1b[1;2P".to_vec()));
        // Shift+F5 = CSI 15;2 ~
        assert_eq!(
            encode_key(Key::F(5), shift, d),
            Some(b"\x1b[15;2~".to_vec())
        );
    }

    #[test]
    fn test_keypad_app_mode_enter() {
        // En app keypad, Enter del numpad -> SS3 M. Modelamos el numpad Enter como
        // Key::Char('\r') con un flag de procedencia no disponible; por eso este
        // test cubre el caso general: con app_keypad, Enter normal sigue siendo CR.
        let m = KeyModes {
            app_keypad: true,
            ..KeyModes::default()
        };
        assert_eq!(encode_key(Key::Enter, Mods::NONE, m), Some(vec![0x0d]));
    }
}
