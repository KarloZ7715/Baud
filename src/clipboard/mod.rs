//! Clipboard cross-platform con capacidades honestas.
//!
//! Façade estable (`set` / `get` / `set_detached` / `CopyTarget`) sobre un backend
//! seleccionado una vez por proceso: arboard (preferido) y fallback CLI en Unix.

mod arboard_backend;
mod caps;
mod cli_fallback;
mod select;

use std::sync::OnceLock;
use std::thread;

use arboard_backend::ArboardBackend;
use caps::{resolve_primary, Capabilities};
use cli_fallback::CliFallbackBackend;
use select::{select_backend_kind, BackendKind, EnvSnapshot};

pub use caps::Capabilities as ClipboardCapabilities;
pub use select::BackendKind as ClipboardBackendKind;

/// Error de operación de clipboard (interno; la façade soft-fail).
#[derive(Debug)]
pub struct ClipboardError(pub String);

/// Contrato de un backend de clipboard.
pub trait ClipboardBackend: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn set(&self, text: &str, primary: bool) -> Result<(), ClipboardError>;
    fn get(&self, primary: bool) -> Result<String, ClipboardError>;
}

/// Backend nulo: sin clipboard real.
struct NullBackend;

impl ClipboardBackend for NullBackend {
    fn capabilities(&self) -> Capabilities {
        Capabilities::NONE
    }

    fn set(&self, _text: &str, _primary: bool) -> Result<(), ClipboardError> {
        Err(ClipboardError("clipboard no disponible".into()))
    }

    fn get(&self, _primary: bool) -> Result<String, ClipboardError> {
        Err(ClipboardError("clipboard no disponible".into()))
    }
}

/// Híbrido: intenta arboard y, si falla, el fallback CLI.
struct HybridBackend {
    arboard: Option<ArboardBackend>,
    cli: Option<CliFallbackBackend>,
    caps: Capabilities,
}

impl ClipboardBackend for HybridBackend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn set(&self, text: &str, primary: bool) -> Result<(), ClipboardError> {
        if let Some(ref arb) = self.arboard {
            match arb.set(text, primary) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("clipboard: arboard set falló ({}); probando CLI", e.0);
                }
            }
        }
        if let Some(ref cli) = self.cli {
            return cli.set(text, primary);
        }
        Err(ClipboardError(
            "ningún backend de clipboard disponible".into(),
        ))
    }

    fn get(&self, primary: bool) -> Result<String, ClipboardError> {
        let mut arboard_empty = None;
        if let Some(ref arb) = self.arboard {
            match arb.get(primary) {
                Ok(text) if !text.is_empty() => return Ok(text),
                Ok(empty) => {
                    // Vacío: el set puede haber caído al CLI; reintentar ahí.
                    arboard_empty = Some(empty);
                }
                Err(e) => {
                    tracing::warn!("clipboard: arboard get falló ({}); probando CLI", e.0);
                }
            }
        }
        if let Some(ref cli) = self.cli {
            match cli.get(primary) {
                Ok(text) if !text.is_empty() => return Ok(text),
                Ok(empty) => {
                    if arboard_empty.is_none() {
                        return Ok(empty);
                    }
                }
                Err(e) => {
                    if arboard_empty.is_none() && self.arboard.is_none() {
                        return Err(e);
                    }
                    tracing::debug!("clipboard: CLI get falló ({})", e.0);
                }
            }
        }
        if let Some(empty) = arboard_empty {
            return Ok(empty);
        }
        Err(ClipboardError(
            "ningún backend de clipboard disponible".into(),
        ))
    }
}

static BACKEND: OnceLock<Box<dyn ClipboardBackend>> = OnceLock::new();

fn host() -> &'static dyn ClipboardBackend {
    BACKEND.get_or_init(init_backend).as_ref()
}

fn init_backend() -> Box<dyn ClipboardBackend> {
    let env = EnvSnapshot::from_process();
    let kind = select_backend_kind(&env);
    match kind {
        BackendKind::Null => {
            tracing::warn!("clipboard: sin WAYLAND_DISPLAY/DISPLAY ni Windows — NullBackend");
            Box::new(NullBackend)
        }
        BackendKind::Windows => match ArboardBackend::try_new(Capabilities::CLIPBOARD_ONLY) {
            Ok(arb) => Box::new(HybridBackend {
                arboard: Some(arb),
                cli: None,
                caps: Capabilities::CLIPBOARD_ONLY,
            }),
            Err(e) => {
                tracing::warn!("clipboard: arboard init falló en Windows: {}", e.0);
                Box::new(NullBackend)
            }
        },
        BackendKind::Wayland | BackendKind::X11 => {
            let arboard = match ArboardBackend::try_new(Capabilities::FULL) {
                Ok(arb) => Some(arb),
                Err(e) => {
                    tracing::warn!(
                        "clipboard: arboard init falló ({}); usando fallback CLI",
                        e.0
                    );
                    None
                }
            };
            Box::new(HybridBackend {
                arboard,
                cli: Some(CliFallbackBackend::new()),
                caps: Capabilities::FULL,
            })
        }
    }
}

/// Capacidades del backend activo.
pub fn capabilities() -> Capabilities {
    host().capabilities()
}

/// Carriles de escritura para un `CopyTarget` dadas las capacidades.
fn write_lanes(target: CopyTarget, caps: Capabilities) -> Vec<bool> {
    match target {
        CopyTarget::Clipboard => vec![false],
        CopyTarget::Primary => vec![resolve_primary(true, caps)],
        CopyTarget::Both => {
            if caps.primary {
                vec![false, true]
            } else {
                // Sin primary: un solo set a clipboard (no duplicar).
                vec![false]
            }
        }
    }
}

/// Copia `text` al clipboard o a la primary selection (remap si no hay primary).
pub fn set(text: &str, primary: bool) {
    let caps = host().capabilities();
    if !caps.clipboard {
        tracing::warn!("clipboard::set: backend sin clipboard");
        return;
    }
    let primary = resolve_primary(primary, caps);
    if let Err(e) = host().set(text, primary) {
        tracing::warn!("clipboard::set: {}", e.0);
    }
}

/// Encola el set en un hilo dedicado para no bloquear drain/GUI.
///
/// Usar desde el hilo drain tras liberar `Mutex<Term>` (OSC 52 write).
pub fn set_detached(text: String, primary: bool) {
    let _ = thread::Builder::new()
        .name("baud-clipboard".into())
        .spawn(move || set(&text, primary));
}

/// Lee texto del clipboard o de la primary selection. Vacío si falla.
pub fn get(primary: bool) -> String {
    let caps = host().capabilities();
    if !caps.clipboard {
        tracing::warn!("clipboard::get: backend sin clipboard");
        return String::new();
    }
    let primary = resolve_primary(primary, caps);
    match host().get(primary) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!("clipboard::get: {}", e.0);
            String::new()
        }
    }
}

/// Destino de copy-on-select.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CopyTarget {
    Clipboard,
    Primary,
    Both,
}

impl CopyTarget {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "clipboard" => CopyTarget::Clipboard,
            "both" => CopyTarget::Both,
            _ => CopyTarget::Primary,
        }
    }

    pub fn write(self, text: &str) {
        let caps = host().capabilities();
        for primary in write_lanes(self, caps) {
            // Desacoplado del hilo GUI/drain: set síncrono congela el event loop.
            set_detached(text.to_owned(), primary);
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CopyTarget::Clipboard => "clipboard",
            CopyTarget::Primary => "primary",
            CopyTarget::Both => "clipboard y primary",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use select::{select_backend_kind, BackendKind, EnvSnapshot};

    #[test]
    fn copy_target_label() {
        assert_eq!(CopyTarget::Clipboard.label(), "clipboard");
        assert_eq!(CopyTarget::Primary.label(), "primary");
        assert_eq!(CopyTarget::Both.label(), "clipboard y primary");
    }

    #[test]
    fn copy_target_parse_desconocido_usa_primary() {
        assert_eq!(CopyTarget::parse("bogus"), CopyTarget::Primary);
    }

    #[test]
    fn write_lanes_both_sin_primary_un_solo_clipboard() {
        assert_eq!(
            write_lanes(CopyTarget::Both, Capabilities::CLIPBOARD_ONLY),
            vec![false]
        );
    }

    #[test]
    fn write_lanes_both_con_primary_dos_carriles() {
        assert_eq!(
            write_lanes(CopyTarget::Both, Capabilities::FULL),
            vec![false, true]
        );
    }

    #[test]
    fn write_lanes_primary_sin_caps_va_a_clipboard() {
        assert_eq!(
            write_lanes(CopyTarget::Primary, Capabilities::CLIPBOARD_ONLY),
            vec![false]
        );
    }

    #[test]
    fn copy_target_write_usa_carriles_sin_duplicar_sin_primary() {
        // Contrato: Both sin primary → un solo carril clipboard (el set va detached).
        assert_eq!(
            write_lanes(CopyTarget::Both, Capabilities::CLIPBOARD_ONLY).len(),
            1
        );
        assert_eq!(write_lanes(CopyTarget::Both, Capabilities::FULL).len(), 2);
    }

    #[test]
    fn selector_matriz_ae4() {
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: true,
                display: false,
                windows: false,
            }),
            BackendKind::Wayland
        );
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: false,
                display: true,
                windows: false,
            }),
            BackendKind::X11
        );
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: false,
                display: false,
                windows: true,
            }),
            BackendKind::Windows
        );
        assert_eq!(
            select_backend_kind(&EnvSnapshot {
                wayland_display: false,
                display: false,
                windows: false,
            }),
            BackendKind::Null
        );
    }
}
