//! Cadena de fallback tipografico para TUIs (iconos Nerd, simbolos, emoji).
//!
//! cosmic-text ya incluye `PlatformFallback` (Noto, DejaVu). Anadimos fuentes
//! que se usan comúnmente para iconos y box-drawing sin depender de una sola
//! Nerd Font parcheada.

use glyphon::cosmic_text::{Fallback, FontSystem, PlatformFallback};
use unicode_script::Script;

/// Fallbacks extra tras los especificos por script de `PlatformFallback`.
#[cfg(target_os = "windows")]
const COMMON: &[&str] = &[
    "Segoe UI Symbol",
    "Segoe UI Emoji",
    "Cascadia Mono",
    "Cascadia Code",
    "Consolas",
];

/// Fallbacks extra tras los especificos por script de `PlatformFallback`.
#[cfg(not(target_os = "windows"))]
const COMMON: &[&str] = &[
    "Symbols Nerd Font Mono",
    "MesloLGS Nerd Font Mono",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    "Noto Color Emoji",
    "DejaVu Sans Mono",
    "Liberation Mono",
];

struct TerminalFallbackChain {
    common: Vec<&'static str>,
}

impl Fallback for TerminalFallbackChain {
    fn common_fallback(&self) -> &[&'static str] {
        &self.common
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, script: Script, locale: &str) -> &[&'static str] {
        PlatformFallback.script_fallback(script, locale)
    }
}

fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".to_string())
}

fn family_in_db(db: &glyphon::fontdb::Database, family: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(name, _)| name == family))
}

fn build_common_fallback(
    db: &glyphon::fontdb::Database,
    user_fallback: &[String],
) -> Vec<&'static str> {
    let mut common = Vec::with_capacity(user_fallback.len() + COMMON.len());
    let mut seen = std::collections::HashSet::new();
    for family in user_fallback {
        if !family_in_db(db, family) {
            tracing::warn!("fuente de fallback no encontrada: '{family}'");
            continue;
        }
        if !seen.insert(family.as_str()) {
            continue;
        }
        // ponytail: Box::leak — el FontSystem vive todo el proceso; el trait Fallback exige &'static str.
        let leaked: &'static str = Box::leak(family.clone().into_boxed_str());
        common.push(leaked);
    }
    for &name in COMMON {
        if seen.insert(name) {
            common.push(name);
        }
    }
    common
}

/// FontSystem con fuentes del sistema y cadena de fallback configurable.
pub fn create_font_system_with_fallback(user_fallback: &[String]) -> glyphon::FontSystem {
    let locale = system_locale();
    let mut db = glyphon::fontdb::Database::new();
    let t_scan = std::time::Instant::now();
    db.load_system_fonts();
    tracing::info!(
        "startup: load_system_fonts en {}ms ({} fuentes)",
        t_scan.elapsed().as_millis(),
        db.len()
    );
    let t_fallback = std::time::Instant::now();
    let common = build_common_fallback(&db, user_fallback);
    let fallback = TerminalFallbackChain { common };
    let font_system = FontSystem::new_with_locale_and_db_and_fallback(locale, db, fallback);
    tracing::info!(
        "startup: resolucion de fallback en {}ms",
        t_fallback.elapsed().as_millis()
    );
    font_system
}

/// FontSystem con fuentes del sistema y fallbacks para terminal (tests y defaults).
#[cfg_attr(not(test), allow(dead_code))]
pub fn create_font_system() -> glyphon::FontSystem {
    create_font_system_with_fallback(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_common_fallback_omite_fuentes_ausentes() {
        let db = glyphon::fontdb::Database::new();
        let chain = build_common_fallback(&db, &["Fuente Inexistente XYZ".into()]);
        assert_eq!(chain.len(), COMMON.len());
    }

    #[test]
    fn build_common_fallback_antepone_usuario_y_deduplica_common() {
        let mut db = glyphon::fontdb::Database::new();
        db.load_system_fonts();
        let known = db
            .faces()
            .find_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .expect("se requiere al menos una fuente del sistema");
        let in_common = COMMON.contains(&known.as_str());
        let chain = build_common_fallback(&db, &[known.clone(), known.clone()]);
        assert_eq!(chain.first().copied(), Some(known.as_str()));
        let count = chain.iter().filter(|&&n| n == known.as_str()).count();
        assert_eq!(count, 1, "familia duplicada en la cadena");
        if in_common {
            assert_eq!(chain.len(), COMMON.len());
        } else {
            assert_eq!(chain.len(), COMMON.len() + 1);
        }
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn common_incluye_fallbacks_linux() {
        assert!(COMMON.contains(&"Noto Color Emoji"));
        assert!(COMMON.contains(&"DejaVu Sans Mono"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn common_incluye_fallbacks_windows() {
        assert!(COMMON.contains(&"Segoe UI Emoji"));
        assert!(COMMON.contains(&"Consolas"));
    }

    #[test]
    fn system_locale_no_esta_vacio_y_no_hace_panic() {
        let locale = system_locale();
        assert!(!locale.is_empty());
    }
}
