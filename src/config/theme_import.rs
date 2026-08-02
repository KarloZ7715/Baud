//! Import de paletas de color desde archivos de otros terminales.
//!
//! Sólo se leen campos de color; fuente, padding, keybinds y opacidad se
//! ignoran. Un valor que no parsea como hex se omite sin abortar el import.

use std::path::{Path, PathBuf};

use super::ThemeConfig;
use crate::color::relative_luminance;
use crate::config::ColorScheme;

/// Paleta parcial: sólo los campos presentes en el archivo de origen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedPalette {
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub cursor: Option<String>,
    pub selection_bg: Option<String>,
    pub selection_fg: Option<String>,
    pub black: Option<String>,
    pub red: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
    pub blue: Option<String>,
    pub magenta: Option<String>,
    pub cyan: Option<String>,
    pub white: Option<String>,
    pub bright_black: Option<String>,
    pub bright_red: Option<String>,
    pub bright_green: Option<String>,
    pub bright_yellow: Option<String>,
    pub bright_blue: Option<String>,
    pub bright_magenta: Option<String>,
    pub bright_cyan: Option<String>,
    pub bright_white: Option<String>,
}

impl ImportedPalette {
    /// True si no se extrajo ningún color.
    pub fn is_empty(&self) -> bool {
        self.foreground.is_none()
            && self.background.is_none()
            && self.cursor.is_none()
            && self.selection_bg.is_none()
            && self.selection_fg.is_none()
            && self.black.is_none()
            && self.red.is_none()
            && self.green.is_none()
            && self.yellow.is_none()
            && self.blue.is_none()
            && self.magenta.is_none()
            && self.cyan.is_none()
            && self.white.is_none()
            && self.bright_black.is_none()
            && self.bright_red.is_none()
            && self.bright_green.is_none()
            && self.bright_yellow.is_none()
            && self.bright_blue.is_none()
            && self.bright_magenta.is_none()
            && self.bright_cyan.is_none()
            && self.bright_white.is_none()
    }

    /// Aplica los campos presentes sobre un tema base (preset u otro).
    pub fn apply_to(&self, theme: &mut ThemeConfig) {
        if let Some(v) = &self.foreground {
            theme.foreground.clone_from(v);
        }
        if let Some(v) = &self.background {
            theme.background.clone_from(v);
        }
        if let Some(v) = &self.cursor {
            theme.cursor.clone_from(v);
        }
        if let Some(v) = &self.selection_bg {
            theme.selection_bg = Some(v.clone());
        }
        if let Some(v) = &self.selection_fg {
            theme.selection_fg = Some(v.clone());
        }
        if let Some(v) = &self.black {
            theme.black.clone_from(v);
        }
        if let Some(v) = &self.red {
            theme.red.clone_from(v);
        }
        if let Some(v) = &self.green {
            theme.green.clone_from(v);
        }
        if let Some(v) = &self.yellow {
            theme.yellow.clone_from(v);
        }
        if let Some(v) = &self.blue {
            theme.blue.clone_from(v);
        }
        if let Some(v) = &self.magenta {
            theme.magenta.clone_from(v);
        }
        if let Some(v) = &self.cyan {
            theme.cyan.clone_from(v);
        }
        if let Some(v) = &self.white {
            theme.white.clone_from(v);
        }
        if let Some(v) = &self.bright_black {
            theme.bright_black.clone_from(v);
        }
        if let Some(v) = &self.bright_red {
            theme.bright_red.clone_from(v);
        }
        if let Some(v) = &self.bright_green {
            theme.bright_green.clone_from(v);
        }
        if let Some(v) = &self.bright_yellow {
            theme.bright_yellow.clone_from(v);
        }
        if let Some(v) = &self.bright_blue {
            theme.bright_blue.clone_from(v);
        }
        if let Some(v) = &self.bright_magenta {
            theme.bright_magenta.clone_from(v);
        }
        if let Some(v) = &self.bright_cyan {
            theme.bright_cyan.clone_from(v);
        }
        if let Some(v) = &self.bright_white {
            theme.bright_white.clone_from(v);
        }
    }

    /// Polaridad por luminancia de fondo vs texto (cae a oscuro si falta info).
    pub fn polarity(&self) -> ColorScheme {
        let bg = self
            .background
            .as_deref()
            .and_then(parse_import_hex)
            .unwrap_or((0, 0, 0));
        let fg = self
            .foreground
            .as_deref()
            .and_then(parse_import_hex)
            .unwrap_or((255, 255, 255));
        if relative_luminance(bg) < relative_luminance(fg) {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        }
    }
}

/// Resultado de un import: variantes oscura y/o clara.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportedTheme {
    pub dark: Option<ImportedPalette>,
    pub light: Option<ImportedPalette>,
}

/// Error al leer o interpretar un archivo de tema ajeno.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    Io(String),
    Parse(String),
    Empty,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "{msg}"),
            Self::Parse(msg) => write!(f, "{msg}"),
            Self::Empty => write!(f, "archivo de tema sin colores reconocibles"),
        }
    }
}

/// Formato detectado del archivo de origen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeFormat {
    Foot,
    Ghostty,
    Kitty,
    Alacritty,
}

/// Lee y parsea un archivo de tema ajeno según su nombre/contenido.
pub fn import_from_path(path: &Path) -> Result<ImportedTheme, ImportError> {
    let src = std::fs::read_to_string(path).map_err(|e| ImportError::Io(e.to_string()))?;
    import_from_str(&src, path)
}

/// Parsea el contenido de un archivo de tema ajeno.
pub fn import_from_str(src: &str, path: &Path) -> Result<ImportedTheme, ImportError> {
    let format = detect_format(path, src);
    let imported = match format {
        ThemeFormat::Foot => parse_foot(src)?,
        ThemeFormat::Ghostty => parse_ghostty(src)?,
        ThemeFormat::Kitty => parse_kitty(src)?,
        ThemeFormat::Alacritty => parse_alacritty(src)?,
    };
    if imported.dark.is_none() && imported.light.is_none() {
        return Err(ImportError::Empty);
    }
    Ok(imported)
}

/// Detecta el formato por nombre de archivo y, si no basta, por contenido.
pub fn detect_format(path: &Path, src: &str) -> ThemeFormat {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match name.as_str() {
        "foot.ini" => return ThemeFormat::Foot,
        "ghostty.conf" => return ThemeFormat::Ghostty,
        "kitty.conf" => return ThemeFormat::Kitty,
        "alacritty.toml" => return ThemeFormat::Alacritty,
        _ => {}
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ini" => ThemeFormat::Foot,
        "toml" => ThemeFormat::Alacritty,
        "conf" => {
            if src.lines().any(|l| {
                let t = l.trim();
                t.starts_with("palette") && t.contains('=')
            }) {
                ThemeFormat::Ghostty
            } else {
                ThemeFormat::Kitty
            }
        }
        _ => {
            // Sondeo de último recurso.
            if src.contains("[colors") || src.contains("regular0") {
                ThemeFormat::Foot
            } else if src.contains("palette") && src.contains('=') {
                ThemeFormat::Ghostty
            } else if src.contains("[colors.") || src.contains("[colors]") {
                ThemeFormat::Alacritty
            } else {
                ThemeFormat::Kitty
            }
        }
    }
}

/// Preferencia de archivos dentro de un directorio de tema de escritorio.
pub const DESKTOP_THEME_CANDIDATES: &[&str] =
    &["foot.ini", "ghostty.conf", "kitty.conf", "alacritty.toml"];

/// Directorio de tema activo de Omarchy, si existe.
pub fn omarchy_theme_dir() -> Option<PathBuf> {
    let dir = dirs::config_dir()?
        .join("omarchy")
        .join("current")
        .join("theme");
    dir.is_dir().then_some(dir)
}

/// Elige el primer candidato de tema presente en `dir`.
pub fn pick_desktop_theme_file(dir: &Path) -> Option<PathBuf> {
    for name in DESKTOP_THEME_CANDIDATES {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Rutas a vigilar para un import autodetectado: el enlace `current` de
/// Omarchy (para notar el cambio de tema) y el archivo elegido, resuelto (para
/// notar ediciones directas). `picked_file` es `.../omarchy/current/theme/<archivo>`;
/// su abuelo es el enlace `current`.
///
/// Vigilar sólo el archivo resuelto no basta: `omarchy-theme-set` recrea el
/// enlace `current` apuntando a otro directorio, pero el archivo de destino
/// (p. ej. `foot.ini` del tema nuevo) puede tener un mtime antiguo de cuando
/// se instaló el tema, así que su cambio de identidad no se refleja en su
/// propio mtime.
pub fn omarchy_watch_paths(picked_file: &Path) -> Vec<super::watch::WatchTarget> {
    use super::watch::WatchTarget;
    let mut targets = Vec::new();
    if let Some(current_link) = picked_file.parent().and_then(Path::parent) {
        targets.push(WatchTarget::Link(current_link.to_path_buf()));
    }
    let resolved = std::fs::canonicalize(picked_file).unwrap_or_else(|_| picked_file.to_path_buf());
    targets.push(WatchTarget::File(resolved));
    targets
}

/// Expande `~` y resuelve rutas relativas al directorio del config.
pub fn expand_import_path(raw: &str, config_dir: Option<&Path>) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest);
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else if let Some(dir) = config_dir {
        dir.join(path)
    } else {
        path
    }
}

/// Etiqueta legible para el picker a partir de la ruta importada.
pub fn import_label(path: &Path, autodetected: bool) -> String {
    if autodetected {
        // `.../omarchy/current/theme` suele ser enlace al nombre del tema.
        let theme_name = path
            .parent()
            .and_then(|theme_dir| std::fs::canonicalize(theme_dir).ok())
            .and_then(|canon| {
                canon
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "theme".into());
        format!("Omarchy — {theme_name}")
    } else {
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("import")
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// Parsers por formato
// ---------------------------------------------------------------------------

fn parse_foot(src: &str) -> Result<ImportedTheme, ImportError> {
    let mut sections: Vec<(String, ImportedPalette)> = Vec::new();
    let mut current: Option<(String, ImportedPalette)> = None;

    for raw in src.lines() {
        let line = strip_line_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some((name, pal)) = current.take() {
                if !pal.is_empty() {
                    sections.push((name, pal));
                }
            }
            let name = line[1..line.len() - 1].trim().to_ascii_lowercase();
            current = Some((name, ImportedPalette::default()));
            continue;
        }
        let Some((name, pal)) = current.as_mut() else {
            continue;
        };
        // Sólo secciones de color.
        if !(name == "colors" || name == "colors-dark" || name == "colors-light") {
            continue;
        }
        let Some((key, val)) = split_key_value(line, '=') else {
            continue;
        };
        apply_foot_key(pal, key, val);
    }
    if let Some((name, pal)) = current.take() {
        if !pal.is_empty() {
            sections.push((name, pal));
        }
    }

    let mut out = ImportedTheme::default();
    for (name, pal) in sections {
        match name.as_str() {
            "colors-dark" => out.dark = Some(pal),
            "colors-light" => out.light = Some(pal),
            "colors" => assign_by_polarity(&mut out, pal),
            _ => {}
        }
    }
    Ok(out)
}

fn apply_foot_key(pal: &mut ImportedPalette, key: &str, val: &str) {
    match key {
        "foreground" => pal.foreground = parse_import_hex_str(val),
        "background" => pal.background = parse_import_hex_str(val),
        "selection-foreground" => pal.selection_fg = parse_import_hex_str(val),
        "selection-background" => pal.selection_bg = parse_import_hex_str(val),
        "cursor" => {
            // `cursor=<texto> <cursor>`: Baud sólo tiene color del cursor.
            let mut parts = val.split_whitespace();
            let _text = parts.next();
            if let Some(cursor) = parts.next().or(_text) {
                pal.cursor = parse_import_hex_str(cursor);
            }
        }
        "regular0" => pal.black = parse_import_hex_str(val),
        "regular1" => pal.red = parse_import_hex_str(val),
        "regular2" => pal.green = parse_import_hex_str(val),
        "regular3" => pal.yellow = parse_import_hex_str(val),
        "regular4" => pal.blue = parse_import_hex_str(val),
        "regular5" => pal.magenta = parse_import_hex_str(val),
        "regular6" => pal.cyan = parse_import_hex_str(val),
        "regular7" => pal.white = parse_import_hex_str(val),
        "bright0" => pal.bright_black = parse_import_hex_str(val),
        "bright1" => pal.bright_red = parse_import_hex_str(val),
        "bright2" => pal.bright_green = parse_import_hex_str(val),
        "bright3" => pal.bright_yellow = parse_import_hex_str(val),
        "bright4" => pal.bright_blue = parse_import_hex_str(val),
        "bright5" => pal.bright_magenta = parse_import_hex_str(val),
        "bright6" => pal.bright_cyan = parse_import_hex_str(val),
        "bright7" => pal.bright_white = parse_import_hex_str(val),
        _ => {}
    }
}

fn parse_ghostty(src: &str) -> Result<ImportedTheme, ImportError> {
    let mut pal = ImportedPalette::default();
    for raw in src.lines() {
        let line = strip_line_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, val)) = split_key_value(line, '=') else {
            continue;
        };
        match key {
            "foreground" => pal.foreground = parse_import_hex_str(val),
            "background" => pal.background = parse_import_hex_str(val),
            "cursor-color" => pal.cursor = parse_import_hex_str(val),
            "selection-foreground" => pal.selection_fg = parse_import_hex_str(val),
            "selection-background" => pal.selection_bg = parse_import_hex_str(val),
            "palette" => {
                // `palette = N=#RRGGBB` o `N=#RRGGBB`
                if let Some((idx_s, color)) = val.split_once('=') {
                    if let Ok(idx) = idx_s.trim().parse::<u8>() {
                        set_ansi_index(&mut pal, idx, color.trim());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(single_palette(pal))
}

fn parse_kitty(src: &str) -> Result<ImportedTheme, ImportError> {
    let mut pal = ImportedPalette::default();
    for raw in src.lines() {
        let line = strip_line_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(val) = parts.next() else {
            continue;
        };
        match key {
            "foreground" => pal.foreground = parse_import_hex_str(val),
            "background" => pal.background = parse_import_hex_str(val),
            "cursor" => pal.cursor = parse_import_hex_str(val),
            "selection_foreground" => pal.selection_fg = parse_import_hex_str(val),
            "selection_background" => pal.selection_bg = parse_import_hex_str(val),
            k if k.starts_with("color") => {
                if let Ok(idx) = k[5..].parse::<u8>() {
                    set_ansi_index(&mut pal, idx, val);
                }
            }
            _ => {}
        }
    }
    Ok(single_palette(pal))
}

fn parse_alacritty(src: &str) -> Result<ImportedTheme, ImportError> {
    let value: toml::Value = toml::from_str(src).map_err(|e| ImportError::Parse(e.to_string()))?;
    let mut pal = ImportedPalette::default();
    let Some(colors) = value.get("colors") else {
        return Ok(ImportedTheme::default());
    };

    if let Some(primary) = colors.get("primary") {
        pal.foreground = table_hex(primary, "foreground");
        pal.background = table_hex(primary, "background");
    }
    if let Some(cursor) = colors.get("cursor") {
        pal.cursor = table_hex(cursor, "cursor");
    }
    if let Some(sel) = colors.get("selection") {
        pal.selection_fg = table_hex(sel, "text");
        pal.selection_bg = table_hex(sel, "background");
    }
    if let Some(normal) = colors.get("normal") {
        pal.black = table_hex(normal, "black");
        pal.red = table_hex(normal, "red");
        pal.green = table_hex(normal, "green");
        pal.yellow = table_hex(normal, "yellow");
        pal.blue = table_hex(normal, "blue");
        pal.magenta = table_hex(normal, "magenta");
        pal.cyan = table_hex(normal, "cyan");
        pal.white = table_hex(normal, "white");
    }
    if let Some(bright) = colors.get("bright") {
        pal.bright_black = table_hex(bright, "black");
        pal.bright_red = table_hex(bright, "red");
        pal.bright_green = table_hex(bright, "green");
        pal.bright_yellow = table_hex(bright, "yellow");
        pal.bright_blue = table_hex(bright, "blue");
        pal.bright_magenta = table_hex(bright, "magenta");
        pal.bright_cyan = table_hex(bright, "cyan");
        pal.bright_white = table_hex(bright, "white");
    }
    Ok(single_palette(pal))
}

fn table_hex(table: &toml::Value, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(parse_import_hex_str)
}

fn set_ansi_index(pal: &mut ImportedPalette, idx: u8, raw: &str) {
    let hex = parse_import_hex_str(raw);
    match idx {
        0 => pal.black = hex,
        1 => pal.red = hex,
        2 => pal.green = hex,
        3 => pal.yellow = hex,
        4 => pal.blue = hex,
        5 => pal.magenta = hex,
        6 => pal.cyan = hex,
        7 => pal.white = hex,
        8 => pal.bright_black = hex,
        9 => pal.bright_red = hex,
        10 => pal.bright_green = hex,
        11 => pal.bright_yellow = hex,
        12 => pal.bright_blue = hex,
        13 => pal.bright_magenta = hex,
        14 => pal.bright_cyan = hex,
        15 => pal.bright_white = hex,
        _ => {}
    }
}

fn single_palette(pal: ImportedPalette) -> ImportedTheme {
    if pal.is_empty() {
        return ImportedTheme::default();
    }
    let mut out = ImportedTheme::default();
    assign_by_polarity(&mut out, pal);
    out
}

fn assign_by_polarity(out: &mut ImportedTheme, pal: ImportedPalette) {
    match pal.polarity() {
        ColorScheme::Dark => out.dark = Some(pal),
        ColorScheme::Light => out.light = Some(pal),
    }
}

// ---------------------------------------------------------------------------
// Utilidades de parseo
// ---------------------------------------------------------------------------

fn strip_line_comment(line: &str) -> &str {
    // Comentarios `#` fuera de valores: suficiente para conf/ini de tema.
    match line.find('#') {
        Some(i) if !line[..i].contains('=') && !line[..i].contains('"') => {
            // Línea que empieza por comentario o key sin `=` antes del `#`
            // (p. ej. kitty `color0 #rrggbb` — el `#` es del color).
            // Si el `#` está pegado a un hex de color, no es comentario.
            if i > 0 {
                let prev = line.as_bytes()[i - 1];
                if prev.is_ascii_whitespace() {
                    // Podría ser `key #comment` o `key #rrggbb`.
                    let after = line[i + 1..].trim();
                    if after.len() == 6 && after.chars().all(|c| c.is_ascii_hexdigit())
                        || after.len() == 8 && after.chars().all(|c| c.is_ascii_hexdigit())
                    {
                        return line;
                    }
                    // Comentario real tras un valor no hex: cortar.
                    // Para `key value # comment` con value ya leído por split.
                    return line;
                }
            }
            &line[..i]
        }
        _ => line,
    }
}

fn split_key_value(line: &str, sep: char) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(sep)?;
    let k = k.trim();
    let v = v.trim().trim_matches('"').trim_matches('\'');
    if k.is_empty() || v.is_empty() {
        None
    } else {
        Some((k, v))
    }
}

/// Acepta `#RRGGBB`, `RRGGBB`, `#AARRGGBB` y `AARRGGBB` (descarta alfa).
/// Devuelve `None` si no es hex (nombres de color, basura, etc.).
pub fn parse_import_hex(s: &str) -> Option<(u8, u8, u8)> {
    let t = s.trim().trim_start_matches('#');
    let (r, g, b) = match t.len() {
        6 => (
            u8::from_str_radix(&t[0..2], 16).ok()?,
            u8::from_str_radix(&t[2..4], 16).ok()?,
            u8::from_str_radix(&t[4..6], 16).ok()?,
        ),
        8 => (
            // AARRGGBB → descartar AA
            u8::from_str_radix(&t[2..4], 16).ok()?,
            u8::from_str_radix(&t[4..6], 16).ok()?,
            u8::from_str_radix(&t[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some((r, g, b))
}

fn parse_import_hex_str(s: &str) -> Option<String> {
    let (r, g, b) = parse_import_hex(s)?;
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/themes")
            .join(name)
    }

    fn load(name: &str) -> ImportedTheme {
        import_from_path(&fixture(name)).expect(name)
    }

    fn assert_omarchy_palette(pal: &ImportedPalette) {
        assert_eq!(pal.background.as_deref(), Some("#0c0b0c"));
        assert_eq!(pal.foreground.as_deref(), Some("#fafcfb"));
        assert_eq!(pal.cursor.as_deref(), Some("#e2dddc"));
        assert_eq!(pal.selection_fg.as_deref(), Some("#0c0b0c"));
        assert_eq!(pal.selection_bg.as_deref(), Some("#fafcfb"));
        assert_eq!(pal.black.as_deref(), Some("#0c0b0c"));
        assert_eq!(pal.red.as_deref(), Some("#c38b7b"));
        assert_eq!(pal.green.as_deref(), Some("#87a9b0"));
        assert_eq!(pal.yellow.as_deref(), Some("#6b5e73"));
        assert_eq!(pal.blue.as_deref(), Some("#b59790"));
        assert_eq!(pal.magenta.as_deref(), Some("#c4d8e2"));
        assert_eq!(pal.cyan.as_deref(), Some("#a5a0b6"));
        assert_eq!(pal.white.as_deref(), Some("#cfd3cd"));
        assert_eq!(pal.bright_black.as_deref(), Some("#584e51"));
        assert_eq!(pal.bright_white.as_deref(), Some("#e2dddc"));
    }

    #[test]
    fn parsea_foot_omarchy_como_dark() {
        let t = load("omarchy-foot.ini");
        assert!(t.light.is_none());
        let dark = t.dark.as_ref().expect("dark");
        assert_omarchy_palette(dark);
    }

    #[test]
    fn parsea_ghostty_omarchy() {
        let t = load("omarchy-ghostty.conf");
        assert_omarchy_palette(t.dark.as_ref().expect("dark"));
    }

    #[test]
    fn parsea_kitty_omarchy() {
        let t = load("omarchy-kitty.conf");
        assert_omarchy_palette(t.dark.as_ref().expect("dark"));
    }

    #[test]
    fn parsea_alacritty_omarchy() {
        let t = load("omarchy-alacritty.toml");
        assert_omarchy_palette(t.dark.as_ref().expect("dark"));
    }

    #[test]
    fn los_cuatro_formatos_coinciden() {
        let foot = load("omarchy-foot.ini").dark.unwrap();
        let ghostty = load("omarchy-ghostty.conf").dark.unwrap();
        let kitty = load("omarchy-kitty.conf").dark.unwrap();
        let alacritty = load("omarchy-alacritty.toml").dark.unwrap();
        assert_eq!(foot, ghostty);
        assert_eq!(foot, kitty);
        assert_eq!(foot, alacritty);
    }

    #[test]
    fn foot_acepta_hex_de_8_digitos_sin_alfa() {
        let t = load("foot-8digit.ini");
        let dark = t.dark.as_ref().expect("dark por luminancia");
        assert_eq!(dark.background.as_deref(), Some("#0c0b0c"));
        assert_eq!(dark.cursor.as_deref(), Some("#e2dddc"));
    }

    #[test]
    fn kitty_ignora_nombre_de_color_sin_abortar() {
        let t = load("kitty-named.conf");
        let dark = t.dark.as_ref().expect("dark");
        // `foreground red` se ignora; el resto entra.
        assert!(dark.foreground.is_none());
        assert_eq!(dark.background.as_deref(), Some("#0c0b0c"));
        assert_eq!(dark.cursor.as_deref(), Some("#e2dddc"));
        assert_eq!(dark.black.as_deref(), Some("#0c0b0c"));
    }

    #[test]
    fn archivo_vacio_es_error_empty() {
        let err = import_from_path(&fixture("empty.conf")).unwrap_err();
        assert_eq!(err, ImportError::Empty);
    }

    #[test]
    fn ghostty_truncado_no_panic_y_puede_quedar_parcial() {
        // Línea a medias: el background válido se conserva.
        let t = import_from_path(&fixture("ghostty-truncated.conf")).unwrap();
        let dark = t.dark.as_ref().expect("dark");
        assert_eq!(dark.background.as_deref(), Some("#0c0b0c"));
        assert!(dark.black.is_none());
    }

    #[test]
    fn detecta_formato_por_nombre_y_extension() {
        assert_eq!(detect_format(Path::new("foot.ini"), ""), ThemeFormat::Foot);
        assert_eq!(
            detect_format(Path::new("x.conf"), "palette = 0=#000000\n"),
            ThemeFormat::Ghostty
        );
        assert_eq!(
            detect_format(Path::new("x.conf"), "color0 #000000\n"),
            ThemeFormat::Kitty
        );
        assert_eq!(
            detect_format(Path::new("theme.toml"), ""),
            ThemeFormat::Alacritty
        );
    }

    #[test]
    fn parse_import_hex_variantes() {
        assert_eq!(parse_import_hex("#0c0b0c"), Some((0x0c, 0x0b, 0x0c)));
        assert_eq!(parse_import_hex("0c0b0c"), Some((0x0c, 0x0b, 0x0c)));
        assert_eq!(parse_import_hex("ff0c0b0c"), Some((0x0c, 0x0b, 0x0c)));
        assert_eq!(parse_import_hex("#ff0c0b0c"), Some((0x0c, 0x0b, 0x0c)));
        assert_eq!(parse_import_hex("red"), None);
        assert_eq!(parse_import_hex(""), None);
    }

    #[test]
    fn omarchy_watch_paths_incluye_enlace_current_y_destino_resuelto() {
        let base =
            std::env::temp_dir().join(format!("baud_omarchy_watch_{}_{}", std::process::id(), "a"));
        let real_theme_dir = base.join("themes").join("waves").join("theme");
        std::fs::create_dir_all(&real_theme_dir).unwrap();
        std::fs::write(real_theme_dir.join("foot.ini"), "[colors]\n").unwrap();
        let current_link = base.join("current");
        #[cfg(unix)]
        std::os::unix::fs::symlink(base.join("themes").join("waves"), &current_link).unwrap();
        #[cfg(unix)]
        {
            use crate::config::watch::WatchTarget;
            let picked = current_link.join("theme").join("foot.ini");
            let targets = omarchy_watch_paths(&picked);
            assert_eq!(targets.len(), 2);
            assert_eq!(targets[0], WatchTarget::Link(current_link.clone()));
            assert_eq!(
                targets[1],
                WatchTarget::File(real_theme_dir.join("foot.ini"))
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn expand_tilde_y_relativa() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_import_path("~/foo/bar.ini", None),
            home.join("foo/bar.ini")
        );
        let cfg_dir = Path::new("/tmp/baud-cfg");
        assert_eq!(
            expand_import_path("themes/x.ini", Some(cfg_dir)),
            cfg_dir.join("themes/x.ini")
        );
        assert_eq!(
            expand_import_path("/abs/x.ini", Some(cfg_dir)),
            PathBuf::from("/abs/x.ini")
        );
    }

    #[test]
    fn apply_to_solo_toca_campos_presentes() {
        let mut theme = ThemeConfig::default();
        let original_fg = theme.foreground.clone();
        let pal = ImportedPalette {
            background: Some("#112233".into()),
            ..ImportedPalette::default()
        };
        pal.apply_to(&mut theme);
        assert_eq!(theme.background, "#112233");
        assert_eq!(theme.foreground, original_fg);
    }
}
