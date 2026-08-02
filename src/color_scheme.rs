//! Resolución del modo claro/oscuro del sistema.
//!
//! Detrás de una función única [`system_color_scheme`] hay dos caminos:
//! - **Windows/macOS**: `winit::Window::theme()` + `WindowEvent::ThemeChanged`.
//! - **Linux**: winit 0.30 no implementa color-scheme en su backend Wayland/X11,
//!   así que se lee el portal XDG `org.freedesktop.appearance.color-scheme` y
//!   se escucha su señal `SettingChanged` en un hilo propio
//!   ([`spawn_portal_watcher`]).
//!
//! El portal ausente o sin respuesta es `None`, no un error: Baud cae a oscuro
//! en silencio y arranca sin esperar.

use crate::config::ColorScheme;
use winit::window::Window;

/// Consulta sincrónica del esquema de color del sistema vía winit.
///
/// Devuelve `Some` en Windows/macOS (donde winit sí implementa color-scheme) y
/// `None` en Linux (donde winit 0.30 no lo hace — el portal lo resuelve aparte).
pub fn system_color_scheme(window: &Window) -> Option<ColorScheme> {
    match window.theme()? {
        winit::window::Theme::Dark => Some(ColorScheme::Dark),
        winit::window::Theme::Light => Some(ColorScheme::Light),
    }
}

/// Origen del esquema resuelto por el runtime (para mostrar en el theme picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SchemeSource {
    /// Portal `org.freedesktop.appearance` (Linux).
    Portal,
    /// `winit::Window::theme()` / `WindowEvent::ThemeChanged` (Win/Mac).
    Winit,
    /// Sin señal del SO: cae a oscuro.
    #[default]
    Fallback,
}

#[cfg(all(unix, not(target_os = "macos")))]
mod portal {
    use super::ColorScheme;
    use crate::window::UserEvent;
    use winit::event_loop::EventLoopProxy;
    use zbus::zvariant::{OwnedValue, Value};

    const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
    const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
    const PORTAL_IFACE: &str = "org.freedesktop.portal.Settings";
    const NS_APPEARANCE: &str = "org.freedesktop.appearance";
    const KEY_COLOR_SCHEME: &str = "color-scheme";

    /// `u32` del portal: 0 = sin preferencia, 1 = oscuro, 2 = claro.
    fn scheme_from_u32(v: u32) -> Option<ColorScheme> {
        match v {
            1 => Some(ColorScheme::Dark),
            2 => Some(ColorScheme::Light),
            _ => None,
        }
    }

    /// Extrae el `ColorScheme` de un `Value` del portal: `Read` devuelve un
    /// `VARIANT<UINT32>`; `SettingChanged` pasa el valor crudo. Ambos casos.
    fn extract_scheme(value: &Value<'_>) -> Option<ColorScheme> {
        let n = match value {
            Value::Value(inner) => match &**inner {
                Value::U32(n) => *n,
                _ => return None,
            },
            Value::U32(n) => *n,
            _ => return None,
        };
        scheme_from_u32(n)
    }

    /// Lee y escucha el portal del esquema de color en el hilo actual.
    ///
    /// Portal ausente o sin respuesta => `debug` y retorno silencioso. Nunca
    /// cuelga el arranque: la lectura inicial es síncrona y la escucha bloquea
    /// solo a este hilo dedicado.
    pub fn watch(proxy: EventLoopProxy<UserEvent>) {
        use zbus::blocking::Connection;

        let conn = match Connection::session() {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("color-scheme: sin bus de sesión: {e}");
                return;
            }
        };
        let portal =
            match zbus::blocking::Proxy::new(&conn, PORTAL_SERVICE, PORTAL_PATH, PORTAL_IFACE) {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("color-scheme: portal Settings inaccesible: {e}");
                    return;
                }
            };

        // Lectura inicial: comunica el esquema actual cuanto antes.
        match read_color_scheme(&portal) {
            Ok(Some(scheme)) => {
                let _ = proxy.send_event(UserEvent::SystemColorScheme(scheme));
            }
            Ok(None) => {
                tracing::debug!("color-scheme: el portal no declara preferencia");
            }
            Err(e) => {
                tracing::debug!("color-scheme: lectura inicial falló: {e}");
            }
        }

        // Escucha de cambios en vivo (bloquea este hilo hasta que el bus cae).
        let signals = match portal.receive_signal("SettingChanged") {
            Ok(it) => it,
            Err(e) => {
                tracing::debug!("color-scheme: no se pudo escuchar SettingChanged: {e}");
                return;
            }
        };
        for msg in signals {
            let Ok((ns, key, value)): Result<(String, String, OwnedValue), _> =
                msg.body().deserialize()
            else {
                continue;
            };
            if ns != NS_APPEARANCE || key != KEY_COLOR_SCHEME {
                continue;
            }
            if let Some(scheme) = extract_scheme(&value) {
                let _ = proxy.send_event(UserEvent::SystemColorScheme(scheme));
            }
        }
    }

    fn read_color_scheme(portal: &zbus::blocking::Proxy<'_>) -> zbus::Result<Option<ColorScheme>> {
        let reply = portal.call_method("Read", &(NS_APPEARANCE, KEY_COLOR_SCHEME))?;
        let value: OwnedValue = reply.body().deserialize()?;
        Ok(extract_scheme(&value))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use zbus::zvariant::Value;

        #[test]
        fn scheme_from_u32_mapea_preferencia() {
            assert_eq!(scheme_from_u32(1), Some(ColorScheme::Dark));
            assert_eq!(scheme_from_u32(2), Some(ColorScheme::Light));
            assert_eq!(scheme_from_u32(0), None);
            assert_eq!(scheme_from_u32(99), None);
        }

        #[test]
        fn extract_scheme_desenvuelve_variante() {
            // `Read` devuelve VARIANT<UINT32>.
            let variant = Value::Value(Box::new(Value::U32(2)));
            assert_eq!(extract_scheme(&variant), Some(ColorScheme::Light));
            // `SettingChanged` pasa el UINT32 crudo.
            let raw = Value::U32(1);
            assert_eq!(extract_scheme(&raw), Some(ColorScheme::Dark));
        }

        #[test]
        fn extract_scheme_rechaza_tipos_no_uint32() {
            let s = Value::Str("1".into());
            assert_eq!(extract_scheme(&s), None);
            let nested_bad = Value::Value(Box::new(Value::Str("x".into())));
            assert_eq!(extract_scheme(&nested_bad), None);
        }
    }
}

/// Arranca el observador del portal XDG en un hilo propio (Linux/BSD).
///
/// Windows/macOS no usan el portal: winit provee `theme()` y `ThemeChanged`.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn spawn_portal_watcher(proxy: winit::event_loop::EventLoopProxy<crate::window::UserEvent>) {
    let _ = std::thread::Builder::new()
        .name("color-scheme-portal".into())
        .spawn(move || portal::watch(proxy));
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn spawn_portal_watcher(_proxy: winit::event_loop::EventLoopProxy<crate::window::UserEvent>) {
    // Windows/macOS: winit ya provee theme() y ThemeChanged; sin portal.
}
