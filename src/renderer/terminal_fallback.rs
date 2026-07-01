//! Cadena de fallback tipografico para TUIs (iconos Nerd, simbolos, emoji).
//!
//! cosmic-text ya incluye `PlatformFallback` (Noto, DejaVu). Anadimos fuentes
//! que se usan comúnmente para iconos y box-drawing sin depender de una sola
//! Nerd Font parcheada.

use glyphon::cosmic_text::{Fallback, FontSystem, PlatformFallback};
use unicode_script::Script;

/// Fallbacks extra tras los especificos por script de `PlatformFallback`.
const COMMON: &[&str] = &[
    "Symbols Nerd Font Mono",
    "MesloLGS Nerd Font Mono",
    "Noto Sans Symbols",
    "Noto Sans Symbols2",
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
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_else(|_| "en-US".to_string())
        .split('.')
        .next()
        .unwrap_or("en-US")
        .to_string()
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
    for family in user_fallback {
        if family_in_db(db, family) {
            let leaked: &'static str = Box::leak(family.clone().into_boxed_str());
            common.push(leaked);
        } else {
            tracing::warn!("fuente de fallback no encontrada: '{family}'");
        }
    }
    common.extend_from_slice(COMMON);
    common
}

/// FontSystem con fuentes del sistema y cadena de fallback configurable.
pub fn create_font_system_with_fallback(user_fallback: &[String]) -> glyphon::FontSystem {
    let locale = system_locale();
    let mut db = glyphon::fontdb::Database::new();
    db.load_system_fonts();
    let common = build_common_fallback(&db, user_fallback);
    let fallback = TerminalFallbackChain { common };
    FontSystem::new_with_locale_and_db_and_fallback(locale, db, fallback)
}

/// FontSystem con fuentes del sistema y fallbacks para terminal (tests y defaults).
#[cfg_attr(not(test), allow(dead_code))]
pub fn create_font_system() -> glyphon::FontSystem {
    create_font_system_with_fallback(&[])
}
