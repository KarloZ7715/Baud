//! First-run consent blocking overlay.
//!
//! Shows a centered modal with explanation and Yes/No buttons. PTY input
//! is blocked until the user chooses. Keys: Y/S = Yes, N = No, Esc/others = ignored.

/// Consent modal text.
pub const CONSENT_TITLE: &str = "Error Reporting (optional)";

pub const CONSENT_BODY: &str = concat!(
    "Baud can send crash data from the emulator to the developers (Sentry):\n",
    "crashes, panics, errors, and technical warnings, plus the version\n",
    "and OS. What you type and your command output are never sent.\n",
    "\n",
    "You must choose an option to continue."
);

pub const CONSENT_BUTTONS: &str = "[ Yes, send reports ]    [ No, thanks ]";

pub const CONSENT_HINT: &str =
    "(You can change this later in config.toml → diagnostics.reporting.enabled)";

/// Fills the consent buffer with the modal text.
pub fn fill_consent_buffer(
    buffer: &mut glyphon::Buffer,
    font_system: &mut glyphon::FontSystem,
    font_family: &str,
    surface_width: f32,
    surface_height: f32,
) {
    let mut text = String::new();
    text.push_str(CONSENT_TITLE);
    text.push_str("\n\n");
    text.push_str(CONSENT_BODY);
    text.push_str("\n\n");
    text.push_str(CONSENT_BUTTONS);
    text.push_str("\n\n");
    text.push_str(CONSENT_HINT);

    let family = glyphon::Family::Name(font_family);
    let attrs = glyphon::Attrs::new().family(family);
    let default_attrs = glyphon::Attrs::new().family(family);

    let rich: Vec<(&str, glyphon::Attrs<'_>)> = vec![(text.as_str(), attrs)];
    buffer.set_rich_text(
        font_system,
        rich,
        &default_attrs,
        glyphon::Shaping::Advanced,
        None,
    );
    buffer.set_size(font_system, Some(surface_width), Some(surface_height));
    buffer.shape_until_scroll(font_system, false);
}
