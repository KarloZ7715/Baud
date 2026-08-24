//! Overrides `-o clave=valor` sobre una config ya cargada.
//!
//! Cada par se interpreta como un documento TOML de un solo camino y se
//! mergea sobre el valor actual. Un par inválido no deshace los anteriores
//! ni impide el arranque: se reporta y se sigue.

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::Config;

/// Error al aplicar un par `clave=valor` de la CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideError {
    /// Par original, tal como llegó en `-o`.
    pub pair: String,
    /// Motivo (clave desconocida, TOML inválido, tipo incorrecto).
    pub message: String,
}

impl Config {
    /// Aplica pares `clave=valor` sobre la config ya cargada.
    ///
    /// Cada par se convierte en un documento TOML mínimo y se mergea; los
    /// errores se devuelven para reportar a stderr sin impedir el arranque.
    pub fn apply_overrides(&mut self, pares: &[String]) -> Vec<OverrideError> {
        let mut errors = Vec::new();
        for par in pares {
            if let Err(err) = self.apply_one(par) {
                errors.push(err);
            }
        }
        self.window.opacity = super::clamp_opacity(self.window.opacity);
        self.font.size = super::clamp_font_size_value(self.font.size);
        self.font.text_contrast = super::clamp_text_contrast(self.font.text_contrast);
        errors
    }

    fn apply_one(&mut self, par: &str) -> Result<(), OverrideError> {
        let (key, toml_val) = parse_pair(par)?;
        let parts: Vec<&str> = key.split('.').collect();
        let json_val = serde_json::to_value(&toml_val).map_err(|e| OverrideError {
            pair: par.to_string(),
            message: e.to_string(),
        })?;

        let schema = serde_json::to_value(Config::default_without_theme_import()).map_err(|e| {
            OverrideError {
                pair: par.to_string(),
                message: e.to_string(),
            }
        })?;
        if !json_path_allowed(&schema, &parts) {
            return Err(OverrideError {
                pair: par.to_string(),
                message: format!("unknown key '{key}'"),
            });
        }

        match parts[0] {
            "theme" => apply_to(&mut self.theme, &parts[1..], json_val, par),
            "font" => apply_to(&mut self.font, &parts[1..], json_val, par),
            "window" => apply_to(&mut self.window, &parts[1..], json_val, par),
            "selection" => apply_to(&mut self.selection, &parts[1..], json_val, par),
            "paste" => apply_to(&mut self.paste, &parts[1..], json_val, par),
            "copy_mode" => apply_to(&mut self.copy_mode, &parts[1..], json_val, par),
            "scrollback" => apply_to(&mut self.scrollback, &parts[1..], json_val, par),
            "cursor" => apply_to(&mut self.cursor, &parts[1..], json_val, par),
            "process" => apply_to(&mut self.process, &parts[1..], json_val, par),
            "notifications" => apply_to(&mut self.notifications, &parts[1..], json_val, par),
            "panes" => apply_to(&mut self.panes, &parts[1..], json_val, par),
            "status" => apply_to(&mut self.status, &parts[1..], json_val, par),
            "diagnostics" => apply_to(&mut self.diagnostics, &parts[1..], json_val, par),
            "debug" => apply_to(&mut self.debug, &parts[1..], json_val, par),
            "render" => apply_to(&mut self.render, &parts[1..], json_val, par),
            "keys" => apply_to(&mut self.keys, &parts[1..], json_val, par),
            "bold_is_bright" if parts.len() == 1 => {
                self.bold_is_bright = deserialize_leaf(json_val, par)?;
                Ok(())
            }
            "allow_osc52_read" if parts.len() == 1 => {
                self.allow_osc52_read = deserialize_leaf(json_val, par)?;
                Ok(())
            }
            "remote_control" if parts.len() == 1 => {
                self.remote_control = deserialize_leaf(json_val, par)?;
                Ok(())
            }
            "shell_integration" if parts.len() == 1 => {
                self.shell_integration = deserialize_leaf(json_val, par)?;
                Ok(())
            }
            other => Err(OverrideError {
                pair: par.to_string(),
                message: format!("unknown key '{other}'"),
            }),
        }
    }
}

fn parse_pair(par: &str) -> Result<(String, toml::Value), OverrideError> {
    let Some((key, value)) = par.split_once('=') else {
        return Err(OverrideError {
            pair: par.to_string(),
            message: "expected key=value".into(),
        });
    };
    let key = key.trim();
    if key.is_empty()
        || key.split('.').any(|p| {
            p.is_empty()
                || !p
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    {
        return Err(OverrideError {
            pair: par.to_string(),
            message: format!("invalid key '{key}'"),
        });
    }
    let doc = format!("__v = {value}");
    let parsed: toml::Table = toml::from_str(&doc).map_err(|e| OverrideError {
        pair: par.to_string(),
        message: format!("invalid TOML value: {e}"),
    })?;
    let toml_val = parsed.get("__v").cloned().ok_or_else(|| OverrideError {
        pair: par.to_string(),
        message: "invalid TOML value".into(),
    })?;
    Ok((key.to_string(), toml_val))
}

fn json_path_allowed(schema: &serde_json::Value, parts: &[&str]) -> bool {
    let mut cur = schema;
    for (i, part) in parts.iter().enumerate() {
        match cur {
            serde_json::Value::Object(map) => {
                if let Some(next) = map.get(*part) {
                    cur = next;
                    continue;
                }
                // Objeto vacío = mapa abierto (`keys`, `process.env`).
                return map.is_empty() && i + 1 == parts.len();
            }
            _ => return false,
        }
    }
    true
}

fn apply_to<T: Serialize + DeserializeOwned>(
    target: &mut T,
    parts: &[&str],
    value: serde_json::Value,
    pair: &str,
) -> Result<(), OverrideError> {
    if parts.is_empty() {
        *target = deserialize_leaf(value, pair)?;
        return Ok(());
    }
    let mut json = serde_json::to_value(&*target).map_err(|e| OverrideError {
        pair: pair.to_string(),
        message: e.to_string(),
    })?;
    json_set(&mut json, parts, value);
    *target = deserialize_leaf(json, pair)?;
    Ok(())
}

fn json_set(root: &mut serde_json::Value, parts: &[&str], value: serde_json::Value) {
    let mut cur = root;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        match cur {
            serde_json::Value::Object(map) => {
                if last {
                    map.insert((*part).to_string(), value);
                    return;
                }
                cur = map
                    .entry((*part).to_string())
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            }
            _ => return,
        }
    }
}

fn deserialize_leaf<T: DeserializeOwned>(
    value: serde_json::Value,
    pair: &str,
) -> Result<T, OverrideError> {
    serde_json::from_value(value).map_err(|e| OverrideError {
        pair: pair.to_string(),
        message: format!("invalid type: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_valido_cambia_el_campo() {
        let mut cfg = Config::default();
        let errs = cfg.apply_overrides(&["window.opacity=0.5".into(), "font.size=13".into()]);
        assert!(errs.is_empty(), "{errs:?}");
        assert!((cfg.window.opacity - 0.5).abs() < f32::EPSILON);
        assert_eq!(cfg.font.size, 13);
    }

    #[test]
    fn clave_desconocida_devuelve_error_y_no_toca_nada() {
        let mut cfg = Config::default();
        let before = cfg.clone();
        let errs = cfg.apply_overrides(&["window.no_existe=1".into()]);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("unknown key"));
        assert_eq!(cfg.window.opacity, before.window.opacity);
        assert_eq!(cfg.font.size, before.font.size);
    }

    #[test]
    fn tipo_invalido_devuelve_error_y_no_toca_nada() {
        let mut cfg = Config::default();
        let before = cfg.font.size;
        let errs = cfg.apply_overrides(&[r#"font.size="grande""#.into()]);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("invalid type"));
        assert_eq!(cfg.font.size, before);
    }

    #[test]
    fn par_invalido_no_impide_los_siguientes() {
        let mut cfg = Config::default();
        let errs = cfg.apply_overrides(&[
            "window.no_existe=1".into(),
            "window.opacity=0.25".into(),
            r#"font.size="x""#.into(),
        ]);
        assert_eq!(errs.len(), 2);
        assert!((cfg.window.opacity - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn sin_igual_es_error() {
        let mut cfg = Config::default();
        let errs = cfg.apply_overrides(&["window.opacity".into()]);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("expected key=value"));
    }
}
