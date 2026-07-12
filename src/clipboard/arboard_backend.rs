//! Backend in-process vía arboard (instancia longeva).

use std::sync::Mutex;

use super::caps::Capabilities;
use super::{ClipboardBackend, ClipboardError};

/// Backend arboard: mantiene el `Clipboard` vivo para no perder el contenido en Linux.
pub struct ArboardBackend {
    clipboard: Mutex<arboard::Clipboard>,
    caps: Capabilities,
}

impl ArboardBackend {
    pub fn try_new(caps: Capabilities) -> Result<Self, ClipboardError> {
        let clipboard =
            arboard::Clipboard::new().map_err(|e| ClipboardError(format!("arboard init: {e}")))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
            caps,
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, arboard::Clipboard>, ClipboardError> {
        self.clipboard
            .lock()
            .map_err(|_| ClipboardError("arboard mutex envenenado".into()))
    }
}

impl ClipboardBackend for ArboardBackend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn set(&self, text: &str, primary: bool) -> Result<(), ClipboardError> {
        set_text(&mut *self.lock()?, text, primary)
    }

    fn get(&self, primary: bool) -> Result<String, ClipboardError> {
        get_text(&mut *self.lock()?, primary)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn set_text(cb: &mut arboard::Clipboard, text: &str, primary: bool) -> Result<(), ClipboardError> {
    use arboard::{LinuxClipboardKind, SetExtLinux};
    let kind = if primary {
        LinuxClipboardKind::Primary
    } else {
        LinuxClipboardKind::Clipboard
    };
    cb.set()
        .clipboard(kind)
        .text(text.to_owned())
        .map_err(|e| ClipboardError(format!("arboard set: {e}")))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn get_text(cb: &mut arboard::Clipboard, primary: bool) -> Result<String, ClipboardError> {
    use arboard::{GetExtLinux, LinuxClipboardKind};
    let kind = if primary {
        LinuxClipboardKind::Primary
    } else {
        LinuxClipboardKind::Clipboard
    };
    match cb.get().clipboard(kind).text() {
        Ok(text) => Ok(text),
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(e) => Err(ClipboardError(format!("arboard get: {e}"))),
    }
}

#[cfg(windows)]
fn set_text(cb: &mut arboard::Clipboard, text: &str, primary: bool) -> Result<(), ClipboardError> {
    let _ = primary;
    cb.set_text(text.to_owned())
        .map_err(|e| ClipboardError(format!("arboard set: {e}")))
}

#[cfg(windows)]
fn get_text(cb: &mut arboard::Clipboard, primary: bool) -> Result<String, ClipboardError> {
    let _ = primary;
    match cb.get_text() {
        Ok(text) => Ok(text),
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(e) => Err(ClipboardError(format!("arboard get: {e}"))),
    }
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn set_text(
    _cb: &mut arboard::Clipboard,
    _text: &str,
    _primary: bool,
) -> Result<(), ClipboardError> {
    Err(ClipboardError(
        "arboard no soportado en esta plataforma".into(),
    ))
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn get_text(_cb: &mut arboard::Clipboard, _primary: bool) -> Result<String, ClipboardError> {
    Err(ClipboardError(
        "arboard no soportado en esta plataforma".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_caps_sin_primary() {
        let caps = Capabilities::CLIPBOARD_ONLY;
        assert!(!caps.primary);
        assert!(caps.clipboard);
    }

    #[cfg(windows)]
    #[test]
    fn windows_roundtrip_si_clipboard_libre() {
        let Ok(backend) = ArboardBackend::try_new(Capabilities::CLIPBOARD_ONLY) else {
            return;
        };
        let marker = "baud-clipboard-roundtrip-test";
        if backend.set(marker, false).is_err() {
            // Clipboard bloqueado en CI u otro proceso — no fallar el suite.
            return;
        }
        match backend.get(false) {
            Ok(got) => assert_eq!(got, marker),
            Err(_) => {}
        }
    }
}
