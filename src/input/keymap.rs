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
    /// Flags del protocolo de teclado extendido (CSI u).
    pub keyboard_flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventKind {
    Press,
    Repeat,
    Release,
}

impl KeyEventKind {
    fn code(self) -> u8 {
        match self {
            Self::Press => 1,
            Self::Repeat => 2,
            Self::Release => 3,
        }
    }
}

const KB_FLAG_DISAMBIGUATE: u8 = 1;
const KB_FLAG_REPORT_EVENTS: u8 = 2;
const KB_FLAG_REPORT_ALL: u8 = 8;

/// Codepoint para teclas con representacion en la forma CSI ... u.
fn u_form_codepoint(key: Key) -> Option<u32> {
    Some(match key {
        Key::Char(c) => c.to_ascii_lowercase() as u32,
        Key::Enter => 13,
        Key::Tab => 9,
        Key::Backspace => 127,
        Key::Escape => 27,
        _ => return None,
    })
}

/// Parametro de modificador xterm con subparametro de evento opcional.
fn u_form_mod_field(mods: Mods, event: KeyEventKind, report_events: bool) -> String {
    let m = mods.xterm_param();
    if report_events && event != KeyEventKind::Press {
        format!("{}:{}", m, event.code())
    } else {
        format!("{}", m)
    }
}

/// Encoding CSI <codepoint> ; <mods> u. None => usar encode_key clasico.
///
/// ponytail: flechas, F-keys, PgUp/Del/Home/End no usan forma u; con report-events
/// activo el subparametro de evento no se anexa al encoding clasico. Upgrade path:
/// extender encode_cursor, encode_tilde y encode_fkey con subparametro :2/:3.
pub fn encode_key_extended(
    key: Key,
    mods: Mods,
    modes: KeyModes,
    event: KeyEventKind,
) -> Option<Vec<u8>> {
    let flags = modes.keyboard_flags;
    if flags == 0 {
        return None;
    }
    let report_events = flags & KB_FLAG_REPORT_EVENTS != 0;
    if event == KeyEventKind::Release && !report_events {
        return None;
    }

    let cp = u_form_codepoint(key)?;
    let report_all = flags & KB_FLAG_REPORT_ALL != 0;
    let disambiguate = flags & KB_FLAG_DISAMBIGUATE != 0;

    let plain_text =
        matches!(key, Key::Char(_)) && !mods.any() && event == KeyEventKind::Press && !report_all;
    if plain_text {
        return None;
    }
    if !disambiguate && !report_events && !report_all {
        return None;
    }

    let field = u_form_mod_field(mods, event, report_events);
    Some(format!("\x1b[{};{}u", cp, field).into_bytes())
}

/// Codifica una tecla a bytes para el PTY. None = no se envia nada.
pub fn encode_key(key: Key, mods: Mods, modes: KeyModes) -> Option<Vec<u8>> {
    // ponytail: application keypad real requiere distinguir el numpad fisico, que
    // winit no expone de forma fiable. Se deja el encoding numerico por defecto.
    // Upgrade path: mapear PhysicalKey Numpad* en window.rs y pasar un Key::Numpad*.
    match key {
        Key::Char(c) => Some(encode_char(c, mods)),
        Key::Enter => {
            // Alt antepone ESC (mismo patron que Backspace/Escape). Shift no
            // tiene encoding clasico distinguible de Enter (limitacion del
            // protocolo legacy sin modifyOtherKeys): ambos envian CR/CRLF.
            let base = if modes.newline_mode {
                vec![0x0d, 0x0a]
            } else {
                vec![0x0d]
            };
            Some(if mods.alt {
                let mut out = Vec::with_capacity(base.len() + 1);
                out.push(0x1b);
                out.extend_from_slice(&base);
                out
            } else {
                base
            })
        }
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
    fn test_extended_text_con_ctrl_usa_forma_u() {
        let modes = KeyModes {
            keyboard_flags: 1,
            ..KeyModes::default()
        };
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key_extended(Key::Char('a'), ctrl, modes, KeyEventKind::Press),
            Some(b"\x1b[97;5u".to_vec())
        );
    }

    #[test]
    fn test_extended_text_plano_sin_mods_es_none() {
        let modes = KeyModes {
            keyboard_flags: 1,
            ..KeyModes::default()
        };
        assert_eq!(
            encode_key_extended(Key::Char('a'), Mods::NONE, modes, KeyEventKind::Press),
            None
        );
    }

    #[test]
    fn test_extended_ctrl_enter_disambiguate() {
        let modes = KeyModes {
            keyboard_flags: 1,
            ..KeyModes::default()
        };
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key_extended(Key::Enter, ctrl, modes, KeyEventKind::Press),
            Some(b"\x1b[13;5u".to_vec())
        );
    }

    #[test]
    fn test_extended_repeat_con_report_events() {
        let modes = KeyModes {
            keyboard_flags: 3,
            ..KeyModes::default()
        };
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key_extended(Key::Char('a'), ctrl, modes, KeyEventKind::Repeat),
            Some(b"\x1b[97;5:2u".to_vec())
        );
    }

    #[test]
    fn test_extended_enter_disambiguate() {
        let modes = KeyModes {
            keyboard_flags: 1,
            ..KeyModes::default()
        };
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key_extended(Key::Enter, shift, modes, KeyEventKind::Press),
            Some(b"\x1b[13;2u".to_vec())
        );
    }

    #[test]
    fn test_extended_release_requiere_report_events() {
        let only_disambig = KeyModes {
            keyboard_flags: 1,
            ..KeyModes::default()
        };
        assert_eq!(
            encode_key_extended(
                Key::Char('a'),
                Mods {
                    ctrl: true,
                    ..Mods::NONE
                },
                only_disambig,
                KeyEventKind::Release
            ),
            None
        );
        let with_events = KeyModes {
            keyboard_flags: 3,
            ..KeyModes::default()
        };
        assert_eq!(
            encode_key_extended(
                Key::Char('a'),
                Mods {
                    ctrl: true,
                    ..Mods::NONE
                },
                with_events,
                KeyEventKind::Release
            ),
            Some(b"\x1b[97;5:3u".to_vec())
        );
    }

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
    fn test_ctrl_j_produce_lf_classic_y_kitty() {
        let d = KeyModes::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(encode_key(Key::Char('j'), ctrl, d), Some(vec![0x0a]));

        let disambiguate = KeyModes {
            keyboard_flags: KB_FLAG_DISAMBIGUATE,
            ..KeyModes::default()
        };
        assert_eq!(
            encode_key_extended(Key::Char('j'), ctrl, disambiguate, KeyEventKind::Press),
            Some(b"\x1b[106;5u".to_vec())
        );
    }

    #[test]
    fn test_alt_enter_antepone_esc_classic() {
        let d = KeyModes::default();
        let alt = Mods {
            alt: true,
            ..Mods::NONE
        };
        assert_eq!(encode_key(Key::Enter, alt, d), Some(vec![0x1b, 0x0d]));
    }

    #[test]
    fn test_shift_enter_clasico_es_cr_plano_sin_encoding_distinguible() {
        // Limitacion del protocolo legacy: sin kitty protocol/modifyOtherKeys,
        // ningun terminal puede distinguir Shift+Enter de Enter (ambos 0x0d).
        let d = KeyModes::default();
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(encode_key(Key::Enter, shift, d), Some(vec![0x0d]));
    }

    #[test]
    fn test_enter_chords_kitty_protocol_csi_u() {
        let disambiguate = KeyModes {
            keyboard_flags: KB_FLAG_DISAMBIGUATE,
            ..KeyModes::default()
        };
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        let alt = Mods {
            alt: true,
            ..Mods::NONE
        };
        assert_eq!(
            encode_key_extended(Key::Enter, shift, disambiguate, KeyEventKind::Press),
            Some(b"\x1b[13;2u".to_vec())
        );
        assert_eq!(
            encode_key_extended(Key::Enter, alt, disambiguate, KeyEventKind::Press),
            Some(b"\x1b[13;3u".to_vec())
        );
    }

    #[test]
    fn test_enter_plain_sin_mods_sigue_siendo_cr() {
        // Regresion R7: Enter sin modificadores no debe verse afectado por el
        // prefijo ESC de Alt.
        let d = KeyModes::default();
        assert_eq!(encode_key(Key::Enter, Mods::NONE, d), Some(vec![0x0d]));
    }

    #[test]
    fn test_ctrl_arrow_word_motion_bytes_sin_cambios() {
        // Regresion R7: Ctrl+Arrow no debe verse afectado por los cambios de U1.
        let d = KeyModes::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(encode_key(Key::Right, ctrl, d), Some(b"\x1b[1;5C".to_vec()));
    }

    /// Table-driven: recorre la fila "Chords de nueva linea" de
    /// docs/references/keybinding-matrix.md (columnas Baud clasico/kitty).
    /// Un cambio en esta tabla debe reflejarse tambien en el doc.
    #[test]
    fn test_matrix_newline_chords_table_driven() {
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        let alt = Mods {
            alt: true,
            ..Mods::NONE
        };
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        let d = KeyModes::default();
        let disambiguate = KeyModes {
            keyboard_flags: KB_FLAG_DISAMBIGUATE,
            ..KeyModes::default()
        };

        type MatrixRow = (&'static str, Key, Mods, &'static [u8], &'static [u8]);
        let rows: &[MatrixRow] = &[
            ("ctrl+j", Key::Char('j'), ctrl, &[0x0a], b"\x1b[106;5u"),
            ("alt+enter", Key::Enter, alt, &[0x1b, 0x0d], b"\x1b[13;3u"),
            ("shift+enter", Key::Enter, shift, &[0x0d], b"\x1b[13;2u"),
        ];

        for (label, key, mods, expected_classic, expected_kitty) in rows.iter().copied() {
            assert_eq!(
                encode_key(key, mods, d),
                Some(expected_classic.to_vec()),
                "{label}: encoding clasico no coincide con la matriz"
            );
            assert_eq!(
                encode_key_extended(key, mods, disambiguate, KeyEventKind::Press),
                Some(expected_kitty.to_vec()),
                "{label}: encoding kitty protocol no coincide con la matriz"
            );
        }
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
