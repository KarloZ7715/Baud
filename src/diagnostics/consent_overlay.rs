//! Overlay de consentimiento bloqueante de primer arranque.
//!
//! Muestra un modal centrado con explicación y botones Sí/No. El input del PTY
//! se bloquea hasta que el usuario elige. Teclas: Y/S = Sí, N = No, Esc/otras = ignoradas.

/// Texto del modal de consentimiento.
pub const CONSENT_TITLE: &str = "Informes de error (opcional)";

pub const CONSENT_BODY: &str = concat!(
    "Baud puede enviar a los desarrolladores (Sentry) datos de fallos del\n",
    "emulador: crashes, panics, errores y avisos técnicos, más versión y\n",
    "sistema. No se envía lo que escribes ni la salida de tus comandos.\n",
    "\n",
    "Debes elegir una opción para continuar."
);

pub const CONSENT_BUTTONS: &str = "[ Sí, enviar informes ]    [ No, gracias ]";

pub const CONSENT_HINT: &str =
    "(Puedes cambiarlo después en config.toml → diagnostics.reporting.enabled)";

/// Configura el buffer de consentimiento con el texto del modal.
/// El buffer se rellena con el título, cuerpo, botones y hint, centrados.
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
