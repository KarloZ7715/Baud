//! First-run consent blocking overlay.
//!
//! Shows a centered modal with explanation and Yes/No options. PTY input
//! is blocked until the user chooses. Keys: Y = Yes, N = No. Others ignored.

/// Consent modal text. Follows informed-consent best practices:
/// transparent about what is sent, explicit about what is not, simple language.
pub const CONSENT_TITLE: &str = "Help Improve Baud";

pub const CONSENT_BODY: &str = concat!(
    "Baud can automatically send crash and error reports when something\n",
    "goes wrong. These reports help the developers identify and fix bugs\n",
    "faster, making the terminal more reliable for everyone.\n",
    "\n",
    "Reports include: the type of error, a technical stack trace, and\n",
    "basic system information such as your OS version.\n",
    "\n",
    "Reports do NOT include: anything you type in the terminal, the\n",
    "output of your commands, or any personal data.\n",
    "\n",
    "Reports are sent to Sentry, an industry-standard error tracking\n",
    "service used by the Baud project.\n",
    "\n",
    "You can change this decision at any time in the configuration file."
);

pub const CONSENT_BUTTONS: &str = "[ Yes, send crash reports ]    [ No, thanks ]";

pub const CONSENT_HINT: &str =
    "You can change this later: config.toml → [diagnostics.reporting] enabled = true/false";

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
