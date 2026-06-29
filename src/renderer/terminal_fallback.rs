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

pub struct TerminalFallback;

impl Fallback for TerminalFallback {
    fn common_fallback(&self) -> &[&'static str] {
        COMMON
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

/// FontSystem con fuentes del sistema y fallbacks para terminal.
pub fn create_font_system() -> glyphon::FontSystem {
    let locale = system_locale();
    let mut db = glyphon::fontdb::Database::new();
    db.load_system_fonts();
    FontSystem::new_with_locale_and_db_and_fallback(locale, db, TerminalFallback)
}
