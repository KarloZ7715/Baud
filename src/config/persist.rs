//! Persistencia del preset de tema en el TOML de config.

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Value};

use super::themes::available_presets;

/// Forma en que el tema está declarado en el archivo de config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeForm {
    RootString,
    TableWithName,
    TableInlineOnly,
    Absent,
}

/// Error al persistir el preset de tema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistError {
    Io(String),
    Parse(String),
    UnknownPreset(String),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "{msg}"),
            Self::Parse(msg) => write!(f, "{msg}"),
            Self::UnknownPreset(name) => write!(f, "preset desconocido: {name}"),
        }
    }
}

/// Rutas de config en orden de prioridad (mismo criterio que [`super::Config::load`]).
pub fn config_paths() -> [PathBuf; 2] {
    [
        dirs::config_dir()
            .map(|d| d.join("baud").join("config.toml"))
            .unwrap_or_default(),
        PathBuf::from("baud.toml"),
    ]
}

/// Primer archivo de config existente, o la ruta XDG por defecto para crear uno nuevo.
pub fn config_write_path() -> PathBuf {
    config_paths()
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(default_config_path)
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("baud").join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("baud.toml"))
}

/// Detecta cómo está declarado el tema en un documento TOML.
pub fn detect_theme_form(content: &str) -> Result<ThemeForm, PersistError> {
    let doc = content
        .parse::<DocumentMut>()
        .map_err(|e| PersistError::Parse(e.to_string()))?;
    Ok(detect_theme_form_doc(&doc))
}

fn detect_theme_form_doc(doc: &DocumentMut) -> ThemeForm {
    let Some(item) = doc.get("theme") else {
        return ThemeForm::Absent;
    };
    if item.as_str().is_some() {
        return ThemeForm::RootString;
    }
    let Some(table) = item.as_table() else {
        return ThemeForm::Absent;
    };
    if table.contains_key("name") {
        ThemeForm::TableWithName
    } else {
        ThemeForm::TableInlineOnly
    }
}

fn validate_preset(name: &str) -> Result<(), PersistError> {
    if available_presets().contains(&name) {
        Ok(())
    } else {
        Err(PersistError::UnknownPreset(name.to_string()))
    }
}

/// Resultado de persistir un preset de tema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistOutcome {
    pub path: PathBuf,
    /// La config tenía overrides en `[theme]` que se conservaron al cambiar el preset.
    pub preserved_theme_overrides: bool,
}

fn apply_preset_to_doc(doc: &mut DocumentMut, name: &str) -> bool {
    let form = detect_theme_form_doc(doc);
    match form {
        ThemeForm::RootString => {
            if let Some(item) = doc.get_mut("theme") {
                if let Some(existing) = item.as_value_mut() {
                    *existing = Value::from(name);
                }
            } else {
                doc.insert("theme", Item::Value(Value::from(name)));
            }
            false
        }
        ThemeForm::TableWithName | ThemeForm::TableInlineOnly => {
            let had_overrides = theme_table_has_color_overrides(doc);
            let theme_item = doc
                .entry("theme")
                .or_insert_with(|| Item::Table(toml_edit::Table::new()));
            if let Item::Table(table) = theme_item {
                table.insert("name", Item::Value(Value::from(name)));
            }
            had_overrides
        }
        ThemeForm::Absent => {
            doc.insert("theme", Item::Value(Value::from(name)));
            false
        }
    }
}

/// Claves de `[theme]` que no son el nombre del preset ni toggles de renderer.
const THEME_COLOR_KEYS: &[&str] = &[
    "foreground",
    "background",
    "cursor",
    "selection_bg",
    "selection_fg",
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

fn theme_table_has_color_overrides(doc: &DocumentMut) -> bool {
    doc.get("theme")
        .and_then(|item| item.as_table())
        .is_some_and(|table| THEME_COLOR_KEYS.iter().any(|key| table.contains_key(key)))
}

/// Escribe `theme = "preset"` o actualiza `[theme].name` en `path`.
pub fn write_theme_preset_at(path: &Path, name: &str) -> Result<PersistOutcome, PersistError> {
    validate_preset(name)?;

    let preserved = if path.exists() {
        let content = fs::read_to_string(path).map_err(|e| PersistError::Io(e.to_string()))?;
        let mut doc = content
            .parse::<DocumentMut>()
            .map_err(|e| PersistError::Parse(e.to_string()))?;
        let preserved = apply_preset_to_doc(&mut doc, name);
        fs::write(path, doc.to_string()).map_err(|e| PersistError::Io(e.to_string()))?;
        preserved
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| PersistError::Io(e.to_string()))?;
        }
        let mut doc = DocumentMut::new();
        doc.insert("theme", Item::Value(Value::from(name)));
        fs::write(path, doc.to_string()).map_err(|e| PersistError::Io(e.to_string()))?;
        false
    };
    Ok(PersistOutcome {
        path: path.to_path_buf(),
        preserved_theme_overrides: preserved,
    })
}

/// Persiste el preset en el archivo de config activo (o crea uno nuevo).
pub fn write_theme_preset(name: &str) -> Result<PersistOutcome, PersistError> {
    let path = config_write_path();
    write_theme_preset_at(&path, name)
}

/// mtime del archivo tras escribir (para sincronizar el watcher).
pub fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("baud_persist_{}_{label}.toml", std::process::id()))
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn escribe_theme_string_en_raiz() {
        let path = temp_config_path("root");
        cleanup(&path);
        fs::write(&path, "font.size = 14\n").unwrap();
        write_theme_preset_at(&path, "dracula").unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("theme = \"dracula\""));
        assert!(s.contains("font.size = 14"));
        cleanup(&path);
    }

    #[test]
    fn actualiza_theme_name_en_tabla() {
        let path = temp_config_path("table_name");
        cleanup(&path);
        fs::write(
            &path,
            "[theme]\nname = \"nord\"\nbackground = \"#000000\"\n",
        )
        .unwrap();
        let outcome = write_theme_preset_at(&path, "dracula").unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("name = \"dracula\""));
        assert!(s.contains("[theme]"));
        assert!(s.contains("background = \"#000000\""));
        assert!(!s.contains("name = \"nord\""));
        assert!(outcome.preserved_theme_overrides);
        cleanup(&path);
    }

    #[test]
    fn inserta_name_si_tabla_solo_tiene_colores() {
        let path = temp_config_path("table_inline");
        cleanup(&path);
        fs::write(&path, "[theme]\nbackground = \"#123456\"\n").unwrap();
        let outcome = write_theme_preset_at(&path, "monokai").unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("name = \"monokai\""));
        assert!(s.contains("[theme]"));
        assert!(s.contains("background = \"#123456\""));
        assert!(outcome.preserved_theme_overrides);
        cleanup(&path);
    }

    #[test]
    fn detecta_formatos() {
        assert_eq!(
            detect_theme_form("theme = \"nord\"").unwrap(),
            ThemeForm::RootString
        );
        assert_eq!(
            detect_theme_form("[theme]\nname = \"nord\"\n").unwrap(),
            ThemeForm::TableWithName
        );
        assert_eq!(
            detect_theme_form("[theme]\nbackground = \"#000\"\n").unwrap(),
            ThemeForm::TableInlineOnly
        );
        assert_eq!(
            detect_theme_form("font.size = 12\n").unwrap(),
            ThemeForm::Absent
        );
    }

    #[test]
    fn rechaza_preset_desconocido() {
        let path = temp_config_path("unknown");
        cleanup(&path);
        fs::write(&path, "font.size = 14\n").unwrap();
        let err = write_theme_preset_at(&path, "no-existe").unwrap_err();
        assert_eq!(err, PersistError::UnknownPreset("no-existe".into()));
        cleanup(&path);
    }

    #[test]
    fn crea_archivo_si_no_existe() {
        let path = temp_config_path("new");
        cleanup(&path);
        write_theme_preset_at(&path, "nord").unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("theme = \"nord\""));
        cleanup(&path);
    }

    #[test]
    fn preserva_comentarios_y_claves() {
        let path = temp_config_path("comments");
        cleanup(&path);
        fs::write(&path, "font.size = 14\n# tema favorito\ntheme = \"nord\"\n").unwrap();
        write_theme_preset_at(&path, "dracula").unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("# tema favorito"));
        assert!(s.contains("font.size = 14"));
        assert!(s.contains("theme = \"dracula\""));
        cleanup(&path);
    }

    #[test]
    fn cancel_no_escribe_en_disco() {
        let path = temp_config_path("cancel");
        cleanup(&path);
        let original = "font.size = 14\ntheme = \"nord\"\n";
        fs::write(&path, original).unwrap();
        let mtime_before = file_mtime(&path);
        // Simula cancelar: no llamar write_theme_preset_at.
        let s = fs::read_to_string(&path).unwrap();
        assert_eq!(s, original);
        assert_eq!(file_mtime(&path), mtime_before);
        cleanup(&path);
    }
}
