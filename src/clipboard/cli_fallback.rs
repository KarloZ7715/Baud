//! Fallback por herramientas de sistema: wl-* y luego xclip/xsel.

use std::io::Write;
use std::process::{Command, Stdio};

use super::caps::Capabilities;
use super::{ClipboardBackend, ClipboardError};

/// Backend CLI: no requiere bundling; solo usa binarios presentes en PATH.
pub struct CliFallbackBackend {
    caps: Capabilities,
}

impl CliFallbackBackend {
    pub fn new() -> Self {
        Self {
            caps: Capabilities::FULL,
        }
    }
}

impl Default for CliFallbackBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBackend for CliFallbackBackend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn set(&self, text: &str, primary: bool) -> Result<(), ClipboardError> {
        if try_wl_copy(text, primary) {
            return Ok(());
        }
        if try_xclip_set(text, primary) {
            return Ok(());
        }
        if try_xsel_set(text, primary) {
            return Ok(());
        }
        Err(ClipboardError(
            "ninguna herramienta de clipboard disponible (wl-copy/xclip/xsel)".into(),
        ))
    }

    fn get(&self, primary: bool) -> Result<String, ClipboardError> {
        if let Some(text) = try_wl_paste(primary) {
            return Ok(text);
        }
        if let Some(text) = try_xclip_get(primary) {
            return Ok(text);
        }
        if let Some(text) = try_xsel_get(primary) {
            return Ok(text);
        }
        Err(ClipboardError(
            "ninguna herramienta de clipboard disponible (wl-paste/xclip/xsel)".into(),
        ))
    }
}

fn try_wl_copy(text: &str, primary: bool) -> bool {
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    let Ok(mut child) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
        drop(stdin);
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn try_wl_paste(primary: bool) -> Option<String> {
    let mut cmd = Command::new("wl-paste");
    if primary {
        cmd.arg("--primary");
    }
    match cmd.output() {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => None,
    }
}

fn try_xclip_set(text: &str, primary: bool) -> bool {
    let selection = if primary { "primary" } else { "clipboard" };
    let Ok(mut child) = Command::new("xclip")
        .args(["-selection", selection])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
        drop(stdin);
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn try_xclip_get(primary: bool) -> Option<String> {
    let selection = if primary { "primary" } else { "clipboard" };
    match Command::new("xclip")
        .args(["-selection", selection, "-o"])
        .output()
    {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => None,
    }
}

fn try_xsel_set(text: &str, primary: bool) -> bool {
    let mut cmd = Command::new("xsel");
    if primary {
        cmd.arg("--primary");
    } else {
        cmd.arg("--clipboard");
    }
    cmd.arg("--input");
    let Ok(mut child) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
        drop(stdin);
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn try_xsel_get(primary: bool) -> Option<String> {
    let mut cmd = Command::new("xsel");
    if primary {
        cmd.arg("--primary");
    } else {
        cmd.arg("--clipboard");
    }
    cmd.arg("--output");
    match cmd.output() {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_capabilities_full() {
        let backend = CliFallbackBackend::new();
        assert_eq!(backend.capabilities(), Capabilities::FULL);
    }
}
