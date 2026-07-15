use crate::input::keymap::{Key, Mods};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Copy,
    Paste,
    PastePrimary,
    ToggleCopyMode,
    ToggleSearch,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToBottom,
    JumpToPrevPrompt,
    JumpToNextPrompt,
    FontZoomIn,
    FontZoomOut,
    FontZoomReset,
    ToggleThemePicker,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    GotoTab(u8),
    SplitPane,
    ToggleSplit,
    SwapSplit,
    FocusNextPane,
    FocusPrevPane,
    FocusPaneUp,
    FocusPaneDown,
    FocusPaneLeft,
    FocusPaneRight,
    ClosePane,
    ToggleFpsCounter,
    ExtendSelectionWordLeft,
    ExtendSelectionWordRight,
    ExtendSelectionLineStart,
    ExtendSelectionLineEnd,
    ExtendSelectionViewportStart,
    ExtendSelectionViewportEnd,
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
        let alt_ctrl = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        let alt_shift = Mods {
            alt: true,
            shift: true,
            ..Mods::NONE
        };
        Self {
            bindings: vec![
                (Key::Char('c'), cs, Action::Copy),
                (Key::Char('v'), cs, Action::Paste),
                (Key::Char('x'), cs, Action::ToggleCopyMode),
                (Key::Char('f'), cs, Action::ToggleSearch),
                (Key::Char('='), ctrl, Action::FontZoomIn),
                (Key::Char('-'), ctrl, Action::FontZoomOut),
                (Key::Char('0'), ctrl, Action::FontZoomReset),
                (Key::Char('t'), alt_ctrl, Action::ToggleThemePicker),
                (Key::Char('t'), cs, Action::NewTab),
                (Key::Char('w'), cs, Action::CloseTab),
                (Key::PageDown, ctrl, Action::NextTab),
                (Key::PageUp, ctrl, Action::PrevTab),
                (Key::Up, cs, Action::ScrollLineUp),
                (Key::Down, cs, Action::ScrollLineDown),
                (Key::Up, alt, Action::ScrollPageUp),
                (Key::Down, alt, Action::ScrollPageDown),
                (Key::PageUp, shift, Action::ScrollPageUp),
                (Key::PageDown, shift, Action::ScrollPageDown),
                (Key::PageUp, Mods::NONE, Action::ScrollPageUp),
                (Key::PageDown, Mods::NONE, Action::ScrollPageDown),
                (Key::End, ctrl, Action::ScrollToBottom),
                (Key::Up, alt_ctrl, Action::JumpToPrevPrompt),
                (Key::Down, alt_ctrl, Action::JumpToNextPrompt),
                (Key::Char('d'), cs, Action::SplitPane),
                (Key::Char('|'), cs, Action::ToggleSplit),
                (Key::Char('s'), cs, Action::SwapSplit),
                // Ctrl+Shift+] / Ctrl+Shift+[ (convencion de kitty): ciclar foco de
                // panel. Libera Ctrl+Shift+Left/Right para extender seleccion por
                // palabra (convencion universal de editores/terminales).
                (Key::Char(']'), cs, Action::FocusNextPane),
                (Key::Char('['), cs, Action::FocusPrevPane),
                (Key::Up, alt_shift, Action::FocusPaneUp),
                (Key::Down, alt_shift, Action::FocusPaneDown),
                (Key::Left, alt_shift, Action::FocusPaneLeft),
                (Key::Right, alt_shift, Action::FocusPaneRight),
                (Key::Char('q'), cs, Action::ClosePane),
                (Key::F(12), cs, Action::ToggleFpsCounter),
                (Key::Left, cs, Action::ExtendSelectionWordLeft),
                (Key::Right, cs, Action::ExtendSelectionWordRight),
                (Key::Home, shift, Action::ExtendSelectionLineStart),
                (Key::End, shift, Action::ExtendSelectionLineEnd),
                (Key::Home, cs, Action::ExtendSelectionViewportStart),
                (Key::End, cs, Action::ExtendSelectionViewportEnd),
                (Key::Insert, shift, Action::PastePrimary),
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
        "toggle_search" => Action::ToggleSearch,
        "scroll_line_up" => Action::ScrollLineUp,
        "scroll_line_down" => Action::ScrollLineDown,
        "scroll_page_up" => Action::ScrollPageUp,
        "scroll_page_down" => Action::ScrollPageDown,
        "scroll_to_bottom" => Action::ScrollToBottom,
        "jump_to_prev_prompt" => Action::JumpToPrevPrompt,
        "jump_to_next_prompt" => Action::JumpToNextPrompt,
        "font_zoom_in" => Action::FontZoomIn,
        "font_zoom_out" => Action::FontZoomOut,
        "font_zoom_reset" => Action::FontZoomReset,
        "toggle_theme_picker" => Action::ToggleThemePicker,
        "new_tab" => Action::NewTab,
        "close_tab" => Action::CloseTab,
        "next_tab" => Action::NextTab,
        "prev_tab" => Action::PrevTab,
        s if let Some(n) = s.strip_prefix("goto_tab_").and_then(|d| d.parse().ok()) => {
            Action::GotoTab(n)
        }
        "split_pane" | "split_vertical" | "split_horizontal" => Action::SplitPane,
        "toggle_split" => Action::ToggleSplit,
        "swap_split" => Action::SwapSplit,
        "focus_next_pane" => Action::FocusNextPane,
        "focus_prev_pane" => Action::FocusPrevPane,
        "focus_pane_up" => Action::FocusPaneUp,
        "focus_pane_down" => Action::FocusPaneDown,
        "focus_pane_left" => Action::FocusPaneLeft,
        "focus_pane_right" => Action::FocusPaneRight,
        "close_pane" => Action::ClosePane,
        "toggle_fps_counter" => Action::ToggleFpsCounter,
        "extend_selection_word_left" => Action::ExtendSelectionWordLeft,
        "extend_selection_word_right" => Action::ExtendSelectionWordRight,
        "extend_selection_line_start" => Action::ExtendSelectionLineStart,
        "extend_selection_line_end" => Action::ExtendSelectionLineEnd,
        "extend_selection_viewport_start" => Action::ExtendSelectionViewportStart,
        "extend_selection_viewport_end" => Action::ExtendSelectionViewportEnd,
        _ => return None,
    })
}

/// Normaliza tecla y modificadores antes de consultar el mapa de bindings.
pub fn normalize_binding_key(key: Key, mods: Mods) -> Key {
    match key {
        // Shift desplaza el simbolo del layout (US QWERTY: '='->'+', '['->'{',
        // ']'->'}') antes de que llegue a logical_key; los bindings por
        // defecto usan el simbolo sin desplazar, asi que se normaliza de
        // vuelta. Deben ir antes del brazo generico de minusculas con Ctrl:
        // to_ascii_lowercase() es un no-op sobre estos simbolos y dejaria
        // el match en el brazo de Ctrl sin volver a pasar por este.
        Key::Char('+') => Key::Char('='),
        Key::Char('{') => Key::Char('['),
        Key::Char('}') => Key::Char(']'),
        Key::Char(c) if mods.ctrl => Key::Char(c.to_ascii_lowercase()),
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
        let alt_ctrl = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        assert_eq!(
            kb.lookup(Key::Char('t'), alt_ctrl),
            Some(Action::ToggleThemePicker)
        );
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('t'), cs), Some(Action::NewTab));
    }

    #[test]
    fn default_bindings_tabs() {
        let kb = Keybindings::default();
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('t'), cs), Some(Action::NewTab));
        assert_eq!(kb.lookup(Key::Char('w'), cs), Some(Action::CloseTab));
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::PageDown, ctrl), Some(Action::NextTab));
        assert_eq!(kb.lookup(Key::PageUp, ctrl), Some(Action::PrevTab));
    }

    #[test]
    fn test_default_bindings_pane_splits() {
        let kb = Keybindings::default();
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('d'), cs), Some(Action::SplitPane));
        assert_eq!(kb.lookup(Key::Char('|'), cs), Some(Action::ToggleSplit));
        assert_eq!(kb.lookup(Key::Char('s'), cs), Some(Action::SwapSplit));
        assert_eq!(kb.lookup(Key::Char('t'), cs), Some(Action::NewTab));
        assert_eq!(kb.lookup(Key::Char('e'), cs), None);
        assert_eq!(kb.lookup(Key::Char(']'), cs), Some(Action::FocusNextPane));
        assert_eq!(kb.lookup(Key::Char('['), cs), Some(Action::FocusPrevPane));
        assert_eq!(kb.lookup(Key::Char('q'), cs), Some(Action::ClosePane));
        let alt_shift = Mods {
            alt: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Up, alt_shift), Some(Action::FocusPaneUp));
        assert_eq!(kb.lookup(Key::Down, alt_shift), Some(Action::FocusPaneDown));
    }

    #[test]
    fn test_default_bindings_extend_selection() {
        let kb = Keybindings::default();
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(
            kb.lookup(Key::Left, cs),
            Some(Action::ExtendSelectionWordLeft)
        );
        assert_eq!(
            kb.lookup(Key::Right, cs),
            Some(Action::ExtendSelectionWordRight)
        );
        assert_eq!(
            kb.lookup(Key::Home, shift),
            Some(Action::ExtendSelectionLineStart)
        );
        assert_eq!(
            kb.lookup(Key::End, shift),
            Some(Action::ExtendSelectionLineEnd)
        );
        assert_eq!(
            kb.lookup(Key::Home, cs),
            Some(Action::ExtendSelectionViewportStart)
        );
        assert_eq!(
            kb.lookup(Key::End, cs),
            Some(Action::ExtendSelectionViewportEnd)
        );
        assert_eq!(kb.lookup(Key::Insert, shift), Some(Action::PastePrimary));
    }

    #[test]
    fn test_parse_action_extend_selection_str() {
        assert_eq!(
            parse_action("extend_selection_word_left"),
            Some(Action::ExtendSelectionWordLeft)
        );
        assert_eq!(
            parse_action("extend_selection_word_right"),
            Some(Action::ExtendSelectionWordRight)
        );
        assert_eq!(
            parse_action("extend_selection_line_start"),
            Some(Action::ExtendSelectionLineStart)
        );
        assert_eq!(
            parse_action("extend_selection_line_end"),
            Some(Action::ExtendSelectionLineEnd)
        );
        assert_eq!(
            parse_action("extend_selection_viewport_start"),
            Some(Action::ExtendSelectionViewportStart)
        );
        assert_eq!(
            parse_action("extend_selection_viewport_end"),
            Some(Action::ExtendSelectionViewportEnd)
        );
    }

    #[test]
    fn test_parse_action_pane_str() {
        assert_eq!(parse_action("split_pane"), Some(Action::SplitPane));
        assert_eq!(parse_action("split_vertical"), Some(Action::SplitPane));
        assert_eq!(parse_action("split_horizontal"), Some(Action::SplitPane));
        assert_eq!(parse_action("toggle_split"), Some(Action::ToggleSplit));
        assert_eq!(parse_action("swap_split"), Some(Action::SwapSplit));
        assert_eq!(parse_action("focus_next_pane"), Some(Action::FocusNextPane));
        assert_eq!(parse_action("focus_pane_up"), Some(Action::FocusPaneUp));
        assert_eq!(parse_action("close_pane"), Some(Action::ClosePane));
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
    fn parse_action_reconoce_jump_prompt() {
        assert_eq!(
            parse_action("jump_to_prev_prompt"),
            Some(Action::JumpToPrevPrompt)
        );
        assert_eq!(
            parse_action("jump_to_next_prompt"),
            Some(Action::JumpToNextPrompt)
        );
    }

    #[test]
    fn test_default_bindings_jump_prompt() {
        let kb = Keybindings::default();
        let alt_ctrl = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Up, alt_ctrl), Some(Action::JumpToPrevPrompt));
        assert_eq!(
            kb.lookup(Key::Down, alt_ctrl),
            Some(Action::JumpToNextPrompt)
        );
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
    fn test_normalize_binding_key_llaves_a_corchetes_focus_pane() {
        // Con Shift sostenido, winit reporta el simbolo desplazado del layout
        // ('{'/'}' en US QWERTY), no el corchete sin desplazar almacenado en
        // el binding por defecto. Mismo patron que '+' -> '=' para FontZoomIn.
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let kb = Keybindings::default();
        assert_eq!(
            kb.lookup(normalize_binding_key(Key::Char('{'), cs), cs),
            Some(Action::FocusPrevPane)
        );
        assert_eq!(
            kb.lookup(normalize_binding_key(Key::Char('}'), cs), cs),
            Some(Action::FocusNextPane)
        );
    }

    #[test]
    fn test_normalize_binding_key_mas_a_igual_con_ctrl_sostenido() {
        // Bug latente preexistente: el brazo '+' -> '=' nunca se alcanzaba
        // porque el brazo generico de Ctrl (to_ascii_lowercase, no-op sobre
        // simbolos) iba primero y consumia el match. Ctrl+Shift+= produce
        // '+' en logical_key y debe seguir disparando FontZoomIn (bound a
        // Ctrl+'=' sin shift).
        let ctrl = Mods {
            ctrl: true,
            ..Mods::NONE
        };
        let kb = Keybindings::default();
        assert_eq!(
            kb.lookup(normalize_binding_key(Key::Char('+'), ctrl), ctrl),
            Some(Action::FontZoomIn)
        );
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

    /// Table-driven: recorre la tabla "Bindings por defecto existentes" de
    /// docs/references/keybinding-matrix.md. Un cambio en un default debe
    /// reflejarse tambien en el doc (y viceversa).
    #[test]
    fn test_matrix_default_bindings_table_driven() {
        let kb = Keybindings::default();
        let none = Mods::NONE;
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
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let alt_ctrl = Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        };
        let alt_shift = Mods {
            alt: true,
            shift: true,
            ..Mods::NONE
        };

        let rows: &[(Key, Mods, Action)] = &[
            (Key::Char('c'), cs, Action::Copy),
            (Key::Char('v'), cs, Action::Paste),
            (Key::Insert, shift, Action::PastePrimary),
            (Key::Char('x'), cs, Action::ToggleCopyMode),
            (Key::Char('f'), cs, Action::ToggleSearch),
            (Key::Char('='), ctrl, Action::FontZoomIn),
            (Key::Char('-'), ctrl, Action::FontZoomOut),
            (Key::Char('0'), ctrl, Action::FontZoomReset),
            (Key::Char('t'), alt_ctrl, Action::ToggleThemePicker),
            (Key::Char('t'), cs, Action::NewTab),
            (Key::Char('w'), cs, Action::CloseTab),
            (Key::PageDown, ctrl, Action::NextTab),
            (Key::PageUp, ctrl, Action::PrevTab),
            (Key::Up, cs, Action::ScrollLineUp),
            (Key::Down, cs, Action::ScrollLineDown),
            (Key::Up, alt, Action::ScrollPageUp),
            (Key::Down, alt, Action::ScrollPageDown),
            (Key::PageUp, shift, Action::ScrollPageUp),
            (Key::PageDown, shift, Action::ScrollPageDown),
            (Key::PageUp, none, Action::ScrollPageUp),
            (Key::PageDown, none, Action::ScrollPageDown),
            (Key::End, ctrl, Action::ScrollToBottom),
            (Key::Up, alt_ctrl, Action::JumpToPrevPrompt),
            (Key::Down, alt_ctrl, Action::JumpToNextPrompt),
            (Key::Char('d'), cs, Action::SplitPane),
            (Key::Char('|'), cs, Action::ToggleSplit),
            (Key::Char('s'), cs, Action::SwapSplit),
            (Key::Char(']'), cs, Action::FocusNextPane),
            (Key::Char('['), cs, Action::FocusPrevPane),
            (Key::Up, alt_shift, Action::FocusPaneUp),
            (Key::Down, alt_shift, Action::FocusPaneDown),
            (Key::Left, alt_shift, Action::FocusPaneLeft),
            (Key::Right, alt_shift, Action::FocusPaneRight),
            (Key::Char('q'), cs, Action::ClosePane),
            (Key::F(12), cs, Action::ToggleFpsCounter),
            (Key::Left, cs, Action::ExtendSelectionWordLeft),
            (Key::Right, cs, Action::ExtendSelectionWordRight),
            (Key::Home, shift, Action::ExtendSelectionLineStart),
            (Key::End, shift, Action::ExtendSelectionLineEnd),
            (Key::Home, cs, Action::ExtendSelectionViewportStart),
            (Key::End, cs, Action::ExtendSelectionViewportEnd),
        ];

        for (key, mods, expected) in rows.iter().copied() {
            assert_eq!(
                kb.lookup(key, mods),
                Some(expected),
                "{key:?}+{mods:?} no coincide con la matriz"
            );
        }
    }
}
