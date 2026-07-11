//! Clipboard unificado para Wayland: clipboard y primary selection.
//!
//! Usa `wl-copy`/`wl-paste` (wl-clipboard) vía subprocess. `primary = true`
//! selecciona la selección primaria.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

/// Copia `text` al clipboard o a la primary selection.
pub fn set(text: &str, primary: bool) {
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    let mut child = match cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("clipboard::set: wl-copy no disponible: {e}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
        drop(stdin);
    }
    let _ = child.wait();
}

/// Encola `wl-copy` en un hilo dedicado para no bloquear drain/GUI.
///
/// Usar desde el hilo drain tras liberar `Mutex<Term>` (OSC 52 write).
pub fn set_detached(text: String, primary: bool) {
    let _ = thread::Builder::new()
        .name("baud-clipboard".into())
        .spawn(move || set(&text, primary));
}

/// Lee texto del clipboard o de la primary selection. Vacío si falla.
pub fn get(primary: bool) -> String {
    let mut cmd = Command::new("wl-paste");
    if primary {
        cmd.arg("--primary");
    }
    match cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            tracing::warn!("clipboard::get: wl-paste no disponible");
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
        match self {
            CopyTarget::Clipboard => set(text, false),
            CopyTarget::Primary => set(text, true),
            CopyTarget::Both => {
                set(text, false);
                set(text, true);
            }
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
}
