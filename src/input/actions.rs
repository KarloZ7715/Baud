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
    ScrollToBottom,
    FontZoomIn,
    FontZoomOut,
    FontZoomReset,
    ToggleThemePicker,
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

    /// Construye desde defaults y aplica overrides (combo, action) en texto.
    /// Las entradas invalidas se ignoran con tracing::warn!.
    pub fn from_overrides(overrides: &[(String, String)]) -> Self {
        let mut kb = Keybindings::default();
        for (combo, action) in overrides {
            match (parse_binding(combo), parse_action(action)) {
                (Some((k, m)), Some(a)) => kb.set(k, m, a),
                _ => tracing::warn!("keybinding invalido: '{}' -> '{}'", combo, action),
            }
        }
        kb
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
                (Key::Char('t'), cs, Action::ToggleThemePicker),
                (Key::Up, cs, Action::ScrollLineUp),
                (Key::Down, cs, Action::ScrollLineDown),
                (Key::Up, alt, Action::ScrollPageUp),
                (Key::Down, alt, Action::ScrollPageDown),
                (Key::PageUp, shift, Action::ScrollPageUp),
                (Key::PageDown, shift, Action::ScrollPageDown),
                (Key::PageUp, Mods::NONE, Action::ScrollPageUp),
                (Key::PageDown, Mods::NONE, Action::ScrollPageDown),
                (Key::End, ctrl, Action::ScrollToBottom),
            ],
        }
    }
}

pub fn parse_binding(s: &str) -> Option<(Key, Mods)> {
    if s.is_empty() {
        return None;
    }
    let mut mods = Mods::NONE;
    let parts: Vec<&str> = s.split('+').collect();
    let (key_tok, mod_toks) = parts.split_last()?;
    if key_tok.is_empty() {
        return None;
    }
    for m in mod_toks {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "shift" => mods.shift = true,
            "alt" | "meta" => mods.alt = true,
            "super" | "cmd" => mods.sup = true,
            _ => return None,
        }
    }
    let key = parse_key_token(key_tok)?;
    Some((key, mods))
}

fn parse_key_token(t: &str) -> Option<Key> {
    let lower = t.to_ascii_lowercase();
    Some(match lower.as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        _ => {
            if let Some(n) = lower.strip_prefix('f').and_then(|d| d.parse::<u8>().ok()) {
                if (1..=12).contains(&n) {
                    return Some(Key::F(n));
                }
            }
            let mut chars = t.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Key::Char(c)
        }
    })
}

pub fn parse_action(s: &str) -> Option<Action> {
    Some(match s {
        "copy" => Action::Copy,
        "paste" => Action::Paste,
        "paste_primary" => Action::PastePrimary,
        "toggle_copy_mode" => Action::ToggleCopyMode,
        "scroll_line_up" => Action::ScrollLineUp,
        "scroll_line_down" => Action::ScrollLineDown,
        "scroll_page_up" => Action::ScrollPageUp,
        "scroll_page_down" => Action::ScrollPageDown,
        "scroll_to_bottom" => Action::ScrollToBottom,
        "font_zoom_in" => Action::FontZoomIn,
        "font_zoom_out" => Action::FontZoomOut,
        "font_zoom_reset" => Action::FontZoomReset,
        "toggle_theme_picker" => Action::ToggleThemePicker,
        _ => return None,
    })
}

/// Normaliza tecla y modificadores antes de consultar el mapa de bindings.
pub fn normalize_binding_key(key: Key, mods: Mods) -> Key {
    match key {
        Key::Char(c) if mods.ctrl => Key::Char(c.to_ascii_lowercase()),
        Key::Char('+') => Key::Char('='),
        other => other,
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

    #[test]
    fn test_parse_binding_str() {
        assert_eq!(
            parse_binding("ctrl+shift+c"),
            Some((
                Key::Char('c'),
                Mods {
                    ctrl: true,
                    shift: true,
                    ..Mods::NONE
                }
            ))
        );
        assert_eq!(
            parse_binding("alt+up"),
            Some((
                Key::Up,
                Mods {
                    alt: true,
                    ..Mods::NONE
                }
            ))
        );
        assert_eq!(parse_binding("f5"), Some((Key::F(5), Mods::NONE)));
        assert_eq!(parse_binding(""), None);
        assert_eq!(parse_binding("ctrl+"), None);
    }

    #[test]
    fn test_parse_action_toggle_theme_picker() {
        assert_eq!(
            parse_action("toggle_theme_picker"),
            Some(Action::ToggleThemePicker)
        );
    }

    #[test]
    fn test_default_bindings_theme_picker() {
        let kb = Keybindings::default();
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            kb.lookup(Key::Char('t'), cs),
            Some(Action::ToggleThemePicker)
        );
    }

    #[test]
    fn test_parse_action_str() {
        assert_eq!(parse_action("copy"), Some(Action::Copy));
        assert_eq!(parse_action("font_zoom_in"), Some(Action::FontZoomIn));
        assert_eq!(
            parse_action("scroll_to_bottom"),
            Some(Action::ScrollToBottom)
        );
        assert_eq!(parse_action("desconocida"), None);
    }

    #[test]
    fn test_default_bindings_page_scroll() {
        let kb = Keybindings::default();
        assert_eq!(
            kb.lookup(Key::PageUp, Mods::NONE),
            Some(Action::ScrollPageUp)
        );
        assert_eq!(
            kb.lookup(Key::PageDown, Mods::NONE),
            Some(Action::ScrollPageDown)
        );
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::End, ctrl), Some(Action::ScrollToBottom));
    }

    #[test]
    fn test_normalize_binding_key_uppercase_ctrl() {
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let kb = Keybindings::default();
        let normalized = normalize_binding_key(Key::Char('C'), cs);
        assert_eq!(kb.lookup(normalized, cs), Some(Action::Copy));
    }

    #[test]
    fn test_keybindings_from_overrides_invalid_keeps_default() {
        let overrides = vec![
            ("ctrl+shift+v".to_string(), "paste_primary".to_string()),
            ("mal+combo".to_string(), "copy".to_string()),
        ];
        let kb = Keybindings::from_overrides(&overrides);
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('v'), cs), Some(Action::PastePrimary));
        assert_eq!(kb.lookup(Key::Char('c'), cs), Some(Action::Copy));
    }

    #[test]
    fn test_keybindings_from_overrides() {
        let overrides = vec![("ctrl+shift+v".to_string(), "paste_primary".to_string())];
        let kb = Keybindings::from_overrides(&overrides);
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('v'), cs), Some(Action::PastePrimary));
        assert_eq!(kb.lookup(Key::Char('c'), cs), Some(Action::Copy));
    }
}
