//! Helper para perfil WSL bajo ConPTY.
//!
//! WSL no es un backend de PTY separado: reutiliza ConPTY lanzando
//! `%SystemRoot%\System32\wsl.exe` como proceso hijo. Este módulo construye
//! la línea de comandos y resuelve el ejecutable contra System32 para evitar
//! PATH hijacking.

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
#[cfg(windows)]
use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

/// Resuelve la ruta de `wsl.exe` dentro de System32.
#[cfg(windows)]
pub fn wsl_exe_path() -> io::Result<PathBuf> {
    let mut buf = vec![0u16; 512];
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    if (len as usize) > buf.len() {
        buf.resize(len as usize, 0);
        let len2 = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
        if len2 == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let system_dir = OsString::from_wide(&buf[..len as usize]);
    let mut path = PathBuf::from(system_dir);
    path.push("wsl.exe");
    Ok(path)
}

/// Verifica que el ejecutable WSL exista antes de intentar spawn.
#[cfg(windows)]
pub fn preflight(exe: &Path) -> io::Result<()> {
    if !exe.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("WSL no encontrado: {}", exe.display()),
        ));
    }
    Ok(())
}

/// Construye el argv para `wsl.exe`.
///
/// Forma: `wsl.exe [-d Distro] [--cd Cwd] [--user User] [-- cmd…]`.
/// Para la sesión interactiva por defecto no se añade comando.
pub fn build_wsl_argv(
    distro: Option<&str>,
    cwd: Option<&str>,
    user: Option<&str>,
    command: Option<&[String]>,
) -> Vec<String> {
    let mut argv = Vec::new();
    if let Some(d) = distro {
        argv.push("-d".into());
        argv.push(d.into());
    }
    if let Some(c) = cwd {
        argv.push("--cd".into());
        argv.push(c.into());
    }
    if let Some(u) = user {
        argv.push("--user".into());
        argv.push(u.into());
    }
    if let Some(cmd) = command {
        argv.push("--".into());
        argv.extend(cmd.iter().cloned());
    }
    argv
}

/// Filtra distros de utilidad que no son shells interactivos deseables.
fn is_utility_distro(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("docker-desktop") || lower.starts_with("rancher-desktop")
}

/// Parsea la salida de `wsl -l -q` (una distro por línea) y filtra utilitarias.
pub fn distros_from_output(output: &[u8]) -> Vec<String> {
    let text = decode_wsl_list_output(output);
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !is_utility_distro(l))
        .map(String::from)
        .collect()
}

fn decode_wsl_list_output(output: &[u8]) -> String {
    if output.starts_with(&[0xff, 0xfe]) {
        // UTF-16LE BOM: típico de `wsl -l -q` en consola Windows.
        let u16s: Vec<u16> = output[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(output).into_owned()
    }
}

/// Lista distros WSL disponibles ejecutando `wsl.exe -l -q`.
#[cfg(windows)]
pub fn list_distros() -> io::Result<Vec<String>> {
    let exe = wsl_exe_path()?;
    preflight(&exe)?;
    let out = std::process::Command::new(&exe)
        .args(["-l", "-q"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("wsl -l -q fallo: {}", String::from_utf8_lossy(&out.stderr)),
        ));
    }
    Ok(distros_from_output(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_wsl_argv_no_args() {
        let argv = build_wsl_argv(None, None, None, None);
        assert!(argv.is_empty());
    }

    #[test]
    fn test_build_wsl_argv_distro_cwd_order() {
        let argv = build_wsl_argv(Some("Ubuntu"), Some("~"), None, None);
        assert_eq!(argv, vec!["-d", "Ubuntu", "--cd", "~"]);
    }

    #[test]
    fn test_build_wsl_argv_command_separator() {
        let argv = build_wsl_argv(
            Some("Debian"),
            None,
            Some("carlos"),
            Some(&["bash".into(), "-c".into(), "echo hi".into()]),
        );
        assert_eq!(
            argv,
            vec!["-d", "Debian", "--user", "carlos", "--", "bash", "-c", "echo hi"]
        );
    }

    #[test]
    fn test_distros_filter_utilities() {
        let output = b"Ubuntu\nDebian\ndocker-desktop\nrancher-desktop-data\nopenSUSE\n";
        let distros = distros_from_output(output);
        assert_eq!(distros, vec!["Ubuntu", "Debian", "openSUSE"]);
    }

    #[test]
    fn test_distros_utf16le_bom() {
        let mut bytes = vec![0xff, 0xfe];
        for c in "Ubuntu\r\nDebian\r\n".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let distros = distros_from_output(&bytes);
        assert_eq!(distros, vec!["Ubuntu", "Debian"]);
    }
}
