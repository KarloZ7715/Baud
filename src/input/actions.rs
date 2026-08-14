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

/// Familia de una accion, usada para agrupar el cheatsheet (R13b) por tema
/// en vez de por orden de declaracion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionFamily {
    Scrolling,
    Tabs,
    Panes,
    Selection,
    Search,
    Appearance,
}

impl ActionFamily {
    pub fn label(&self) -> &'static str {
        match self {
            ActionFamily::Scrolling => "Scrolling",
            ActionFamily::Tabs => "Tabs",
            ActionFamily::Panes => "Panes",
            ActionFamily::Selection => "Selection",
            ActionFamily::Search => "Search",
            ActionFamily::Appearance => "Appearance",
        }
    }
}

/// Orden de despliegue de las familias en el cheatsheet.
pub const ACTION_FAMILY_ORDER: &[ActionFamily] = &[
    ActionFamily::Scrolling,
    ActionFamily::Tabs,
    ActionFamily::Panes,
    ActionFamily::Selection,
    ActionFamily::Search,
    ActionFamily::Appearance,
];

impl Action {
    /// Familia tematica de la accion. El match es exhaustivo a proposito y
    /// sin brazo comodin: una accion nueva sin familia asignada no compila
    /// (R13b: "an action with no family assigned is a hard error").
    pub fn family(&self) -> ActionFamily {
        match self {
            Action::ScrollLineUp
            | Action::ScrollLineDown
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollToBottom
            | Action::JumpToPrevPrompt
            | Action::JumpToNextPrompt => ActionFamily::Scrolling,
            Action::NewTab
            | Action::CloseTab
            | Action::NextTab
            | Action::PrevTab
            | Action::GotoTab(_) => ActionFamily::Tabs,
            Action::SplitPane
            | Action::ToggleSplit
            | Action::SwapSplit
            | Action::FocusNextPane
            | Action::FocusPrevPane
            | Action::FocusPaneUp
            | Action::FocusPaneDown
            | Action::FocusPaneLeft
            | Action::FocusPaneRight
            | Action::ClosePane => ActionFamily::Panes,
            Action::Copy
            | Action::Paste
            | Action::PastePrimary
            | Action::ToggleCopyMode
            | Action::ExtendSelectionWordLeft
            | Action::ExtendSelectionWordRight
            | Action::ExtendSelectionLineStart
            | Action::ExtendSelectionLineEnd
            | Action::ExtendSelectionViewportStart
            | Action::ExtendSelectionViewportEnd => ActionFamily::Selection,
            Action::ToggleSearch => ActionFamily::Search,
            Action::FontZoomIn
            | Action::FontZoomOut
            | Action::FontZoomReset
            | Action::ToggleThemePicker
            | Action::ToggleFpsCounter => ActionFamily::Appearance,
        }
    }

    /// Nombre canonico aceptado por [`parse_action`]. Es la inversa de
    /// `parse_action`: para toda `a`, `parse_action(&a.as_str()) == Some(a)`.
    pub fn as_str(&self) -> String {
        match self {
            Action::Copy => "copy".into(),
            Action::Paste => "paste".into(),
            Action::PastePrimary => "paste_primary".into(),
            Action::ToggleCopyMode => "toggle_copy_mode".into(),
            Action::ToggleSearch => "toggle_search".into(),
            Action::ScrollLineUp => "scroll_line_up".into(),
            Action::ScrollLineDown => "scroll_line_down".into(),
            Action::ScrollPageUp => "scroll_page_up".into(),
            Action::ScrollPageDown => "scroll_page_down".into(),
            Action::ScrollToBottom => "scroll_to_bottom".into(),
            Action::JumpToPrevPrompt => "jump_to_prev_prompt".into(),
            Action::JumpToNextPrompt => "jump_to_next_prompt".into(),
            Action::FontZoomIn => "font_zoom_in".into(),
            Action::FontZoomOut => "font_zoom_out".into(),
            Action::FontZoomReset => "font_zoom_reset".into(),
            Action::ToggleThemePicker => "toggle_theme_picker".into(),
            Action::NewTab => "new_tab".into(),
            Action::CloseTab => "close_tab".into(),
            Action::NextTab => "next_tab".into(),
            Action::PrevTab => "prev_tab".into(),
            Action::GotoTab(n) => format!("goto_tab_{n}"),
            Action::SplitPane => "split_pane".into(),
            Action::ToggleSplit => "toggle_split".into(),
            Action::SwapSplit => "swap_split".into(),
            Action::FocusNextPane => "focus_next_pane".into(),
            Action::FocusPrevPane => "focus_prev_pane".into(),
            Action::FocusPaneUp => "focus_pane_up".into(),
            Action::FocusPaneDown => "focus_pane_down".into(),
            Action::FocusPaneLeft => "focus_pane_left".into(),
            Action::FocusPaneRight => "focus_pane_right".into(),
            Action::ClosePane => "close_pane".into(),
            Action::ToggleFpsCounter => "toggle_fps_counter".into(),
            Action::ExtendSelectionWordLeft => "extend_selection_word_left".into(),
            Action::ExtendSelectionWordRight => "extend_selection_word_right".into(),
            Action::ExtendSelectionLineStart => "extend_selection_line_start".into(),
            Action::ExtendSelectionLineEnd => "extend_selection_line_end".into(),
            Action::ExtendSelectionViewportStart => "extend_selection_viewport_start".into(),
            Action::ExtendSelectionViewportEnd => "extend_selection_viewport_end".into(),
        }
    }
}

/// Plataforma a la que esta condicionado un binding por defecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
}

/// Binding por defecto vinculado solo en una plataforma especifica, fuera
/// del listado base de [`Keybindings::default`]. Fuente unica para el runtime
/// (via `#[cfg(windows)]` en `base_bindings`/`Default`) y para el generador
/// de referencia, que debe poder ver el chord de Windows aun compilando en
/// Linux (KTD5).
pub struct ConditionalBinding {
    pub key: Key,
    pub mods: Mods,
    pub action: Action,
    pub platform: Platform,
}

pub const CONDITIONAL_BINDINGS: &[ConditionalBinding] = &[ConditionalBinding {
    key: Key::Char('t'),
    mods: Mods {
        ctrl: true,
        alt: true,
        shift: true,
        sup: false,
    },
    action: Action::ToggleThemePicker,
    platform: Platform::Windows,
}];

/// Letra final del formato canonico de una tecla (inversa de
/// `parse_key_token`, salvo por los alias no canonicos que ese parser acepta
/// ademas del canonico).
fn format_key(key: Key) -> String {
    match key {
        Key::Char(c) => c.to_string(),
        Key::Enter => "enter".into(),
        Key::Tab => "tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Escape => "escape".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::PageUp => "pageup".into(),
        Key::PageDown => "pagedown".into(),
        Key::Insert => "insert".into(),
        Key::Delete => "delete".into(),
        Key::F(n) => format!("f{n}"),
    }
}

/// Formatea una combinacion en la forma canonica que acepta [`parse_binding`]
/// (por ejemplo `"ctrl+shift+c"`). Orden fijo de modificadores: ctrl, alt,
/// shift, super.
pub fn format_binding(key: Key, mods: Mods) -> String {
    let mut s = String::new();
    if mods.ctrl {
        s.push_str("ctrl+");
    }
    if mods.alt {
        s.push_str("alt+");
    }
    if mods.shift {
        s.push_str("shift+");
    }
    if mods.sup {
        s.push_str("super+");
    }
    s.push_str(&format_key(key));
    s
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

    /// Inserta o reemplaza un binding (usado por overrides de config). La tecla
    /// se normaliza al registrarla para que un override escrito en mayuscula
    /// (p. ej. `ctrl+shift+C`) matchee igual que su forma canonica.
    pub fn set(&mut self, key: Key, mods: Mods, action: Action) {
        let key = normalize_binding_key(key, mods);
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
                _ => tracing::warn!("keybinding invalid: '{}' -> '{}'", combo, action),
            }
        }
        kb
    }
}

/// Bindings por defecto validos en todas las plataformas (sin las entradas
/// condicionales de [`CONDITIONAL_BINDINGS`]). Fuente unica compartida por
/// `Keybindings::default` y [`all_default_bindings`].
fn base_bindings() -> Vec<(Key, Mods, Action)> {
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
    vec![
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
    ]
}

impl Default for Keybindings {
    fn default() -> Self {
        #[allow(unused_mut)]
        let mut bindings = base_bindings();
        #[cfg(windows)]
        bindings.extend(
            CONDITIONAL_BINDINGS
                .iter()
                .filter(|c| c.platform == Platform::Windows)
                .map(|c| (c.key, c.mods, c.action)),
        );
        Self { bindings }
    }
}

/// Todas las combinaciones por defecto documentables (base + condicionales de
/// [`CONDITIONAL_BINDINGS`]), con su plataforma (`None` = todas). A
/// diferencia de `Keybindings::default`, no depende del target de
/// compilacion: el generador de referencia ve el chord de Windows aun
/// corriendo en Linux.
pub fn all_default_bindings() -> Vec<(Key, Mods, Action, Option<Platform>)> {
    let mut all: Vec<(Key, Mods, Action, Option<Platform>)> = base_bindings()
        .into_iter()
        .map(|(k, m, a)| (k, m, a, None))
        .collect();
    all.extend(
        CONDITIONAL_BINDINGS
            .iter()
            .map(|c| (c.key, c.mods, c.action, Some(c.platform))),
    );
    all
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

/// Normaliza tecla y modificadores a una forma canonica unica, usada tanto al
/// registrar un binding como al consultarlo. Colapsa las diferencias de
/// plataforma en `logical_key` (mayuscula/minuscula con Shift, caracter de
/// control con Ctrl) para que el mismo chord matchee en Linux y Windows.
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
        // Con Ctrl, algunos backends entregan el caracter de control
        // (\u{1}..\u{1a}) en vez de la letra; se mapea de vuelta a la letra
        // canonica del binding (Ctrl+C -> \u{3} -> 'c').
        Key::Char(c) if mods.ctrl && ('\u{1}'..='\u{1a}').contains(&c) => {
            Key::Char((c as u8 - 1 + b'a') as char)
        }
        // Las letras se guardan en minuscula: con Ctrl o Shift, winit puede
        // entregar la mayuscula (Windows aplica Shift al logical_key).
        Key::Char(c) if mods.ctrl || mods.shift => Key::Char(c.to_ascii_lowercase()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ACTIONS: &[Action] = &[
        Action::Copy,
        Action::Paste,
        Action::PastePrimary,
        Action::ToggleCopyMode,
        Action::ToggleSearch,
        Action::ScrollLineUp,
        Action::ScrollLineDown,
        Action::ScrollPageUp,
        Action::ScrollPageDown,
        Action::ScrollToBottom,
        Action::JumpToPrevPrompt,
        Action::JumpToNextPrompt,
        Action::FontZoomIn,
        Action::FontZoomOut,
        Action::FontZoomReset,
        Action::ToggleThemePicker,
        Action::NewTab,
        Action::CloseTab,
        Action::NextTab,
        Action::PrevTab,
        Action::GotoTab(3),
        Action::SplitPane,
        Action::ToggleSplit,
        Action::SwapSplit,
        Action::FocusNextPane,
        Action::FocusPrevPane,
        Action::FocusPaneUp,
        Action::FocusPaneDown,
        Action::FocusPaneLeft,
        Action::FocusPaneRight,
        Action::ClosePane,
        Action::ToggleFpsCounter,
        Action::ExtendSelectionWordLeft,
        Action::ExtendSelectionWordRight,
        Action::ExtendSelectionLineStart,
        Action::ExtendSelectionLineEnd,
        Action::ExtendSelectionViewportStart,
        Action::ExtendSelectionViewportEnd,
    ];

    /// Round-trip R11: `as_str` es la inversa de `parse_action` para toda accion.
    #[test]
    fn action_as_str_round_trips_through_parse_action() {
        for action in ALL_ACTIONS {
            assert_eq!(
                parse_action(&action.as_str()),
                Some(*action),
                "as_str/parse_action no coinciden para {action:?}"
            );
        }
    }

    /// R13b: cada binding por defecto documentable tiene una familia (el
    /// match exhaustivo de `Action::family` ya lo garantiza en compilacion;
    /// esto fija el mapeo esperado para las acciones mas representativas).
    #[test]
    fn action_family_covers_default_bindings() {
        for (_, _, action, _) in all_default_bindings() {
            let _ = action.family();
        }
        assert_eq!(Action::ScrollLineUp.family(), ActionFamily::Scrolling);
        assert_eq!(Action::NewTab.family(), ActionFamily::Tabs);
        assert_eq!(Action::SplitPane.family(), ActionFamily::Panes);
        assert_eq!(Action::Copy.family(), ActionFamily::Selection);
        assert_eq!(Action::ToggleSearch.family(), ActionFamily::Search);
        assert_eq!(Action::ToggleThemePicker.family(), ActionFamily::Appearance);
    }

    /// Round-trip R11: `format_binding` es la inversa de `parse_binding` para
    /// todo binding por defecto documentable (incluye entradas condicionales).
    #[test]
    fn format_binding_round_trips_through_parse_binding_for_all_default_bindings() {
        for (key, mods, action, _platform) in all_default_bindings() {
            let chord = format_binding(key, mods);
            assert_eq!(
                parse_binding(&chord),
                Some((key, mods)),
                "chord '{chord}' (accion {action:?}) no vuelve a parsear igual"
            );
        }
    }

    /// La tabla de condicionales es la unica fuente: en Windows,
    /// `Keybindings::default` debe contener exactamente esas entradas ademas
    /// de la base; en el resto, ninguna.
    #[test]
    fn conditional_bindings_platform_table_matches_runtime_default() {
        let kb = Keybindings::default();
        for c in CONDITIONAL_BINDINGS {
            let present = kb.lookup(c.key, c.mods) == Some(c.action);
            #[cfg(windows)]
            assert!(present, "binding condicional de Windows ausente en runtime");
            #[cfg(not(windows))]
            assert!(
                !present,
                "binding condicional de Windows presente fuera de Windows"
            );
        }
    }

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
    fn test_theme_picker_windows_dual_binding() {
        let kb = Keybindings::default();
        let alt_ctrl_shift = Mods {
            ctrl: true,
            alt: true,
            shift: true,
            ..Mods::NONE
        };
        #[cfg(windows)]
        assert_eq!(
            kb.lookup(Key::Char('t'), alt_ctrl_shift),
            Some(Action::ToggleThemePicker)
        );
        #[cfg(not(windows))]
        assert_eq!(kb.lookup(Key::Char('t'), alt_ctrl_shift), None);
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
    fn test_normalize_binding_key_control_char_con_ctrl() {
        // Algunos backends entregan el caracter de control (Ctrl+C -> \u{3})
        // en logical_key en vez de la letra; debe mapear de vuelta a 'c'.
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        let kb = Keybindings::default();
        let normalized = normalize_binding_key(Key::Char('\u{3}'), cs);
        assert_eq!(normalized, Key::Char('c'));
        assert_eq!(kb.lookup(normalized, cs), Some(Action::Copy));
    }

    #[test]
    fn test_normalize_binding_key_shift_solo_minuscula() {
        // Shift solo tambien puede entregar la mayuscula en logical_key.
        let shift = Mods {
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(normalize_binding_key(Key::Char('A'), shift), Key::Char('a'));
    }

    #[test]
    fn test_override_en_mayuscula_matchea() {
        // Un override escrito con mayuscula (ctrl+shift+C) debe registrarse
        // normalizado y matchear el mismo chord que la forma canonica.
        let overrides = vec![("ctrl+shift+C".to_string(), "paste".to_string())];
        let kb = Keybindings::from_overrides(&overrides);
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Mods::NONE
        };
        assert_eq!(kb.lookup(Key::Char('c'), cs), Some(Action::Paste));
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
