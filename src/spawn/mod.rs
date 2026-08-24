//! Socket de spawn: un daemon por usuario, no por pid.
//! Independiente de remote_control. Solo hello + new_tab.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod client;
pub mod server;

use std::io;
use std::path::{Path, PathBuf};

use crate::cli::LaunchOptions;
use crate::remote::server::runtime_dir;

/// Parametros de una peticion `new_tab`. Viajan del cliente corto al daemon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpawnParams {
    pub command: Option<Vec<String>>,
    pub working_directory: Option<String>,
    pub title: Option<String>,
    pub hold: bool,
    pub app_id: Option<String>,
}

impl SpawnParams {
    pub fn from_launch(opts: &LaunchOptions) -> Self {
        Self {
            command: opts.command.clone(),
            working_directory: opts.working_directory.clone(),
            title: opts.title.clone(),
            hold: opts.hold,
            app_id: opts.app_id.clone(),
        }
    }
}

/// Resultado de intentar poseer el socket de spawn de este usuario.
pub enum BindOutcome {
    Held(SpawnListener),
    AlreadyHeld,
}

/// Posee el listener exclusivo. Al dropearlo borra sock y token de este proceso.
pub struct SpawnListener {
    #[cfg(unix)]
    listener: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(windows)]
    first_pipe: Option<std::fs::File>,
    dir: PathBuf,
}

impl SpawnListener {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[cfg(unix)]
    pub fn listener(&self) -> &std::os::unix::net::UnixListener {
        &self.listener
    }

    #[cfg(windows)]
    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    #[cfg(windows)]
    pub fn take_first_pipe(&mut self) -> Option<std::fs::File> {
        self.first_pipe.take()
    }
}

impl Drop for SpawnListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(socket_path_in(&self.dir));
        let _ = std::fs::remove_file(token_path_in(&self.dir));
    }
}

pub fn socket_path() -> io::Result<PathBuf> {
    Ok(socket_path_in(&runtime_dir()?))
}

pub fn token_path() -> io::Result<PathBuf> {
    Ok(token_path_in(&runtime_dir()?))
}

pub fn socket_path_in(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        dir.join("spawn.sock")
    }
    #[cfg(windows)]
    {
        dir.join("spawn.pipe")
    }
}

pub fn token_path_in(dir: &Path) -> PathBuf {
    dir.join("spawn.token")
}

pub fn try_bind() -> io::Result<BindOutcome> {
    try_bind_in(&runtime_dir()?)
}

/// Bind exclusivo sobre `dir`. El segundo llamador recibe `AlreadyHeld`.
pub fn try_bind_in(dir: &Path) -> io::Result<BindOutcome> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        try_bind_unix(dir)
    }
    #[cfg(windows)]
    {
        try_bind_windows(dir)
    }
}

#[cfg(unix)]
fn try_bind_unix(dir: &Path) -> io::Result<BindOutcome> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let sock = socket_path_in(dir);
    if sock.exists() {
        match UnixStream::connect(&sock) {
            Ok(_) => return Ok(BindOutcome::AlreadyHeld),
            Err(_) => {
                let _ = std::fs::remove_file(&sock);
                let _ = std::fs::remove_file(token_path_in(dir));
            }
        }
    }
    match UnixListener::bind(&sock) {
        Ok(listener) => {
            let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o700));
            if let Err(e) = listener.set_nonblocking(true) {
                let _ = std::fs::remove_file(&sock);
                return Err(e);
            }
            let held = SpawnListener {
                listener,
                dir: dir.to_path_buf(),
            };
            write_token(dir)?;
            Ok(BindOutcome::Held(held))
        }
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => Ok(BindOutcome::AlreadyHeld),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
fn try_bind_windows(dir: &Path) -> io::Result<BindOutcome> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    const PIPE_NAME: &str = r"\\.\pipe\baud-spawn";
    // ERROR_ACCESS_DENIED / ERROR_PIPE_BUSY: otra instancia ya posee el pipe.
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_PIPE_BUSY: i32 = 231;

    let wide: Vec<u16> =
        std::os::windows::ffi::OsStrExt::encode_wide(std::ffi::OsStr::new(PIPE_NAME))
            .chain(Some(0))
            .collect();
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let err = io::Error::last_os_error();
        return match err.raw_os_error() {
            Some(ERROR_ACCESS_DENIED) | Some(ERROR_PIPE_BUSY) => Ok(BindOutcome::AlreadyHeld),
            _ => Err(err),
        };
    }
    let owned = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
    let held = SpawnListener {
        pipe_name: PIPE_NAME.to_string(),
        first_pipe: Some(std::fs::File::from(owned)),
        dir: dir.to_path_buf(),
    };
    std::fs::write(socket_path_in(dir), PIPE_NAME)?;
    write_token(dir)?;
    Ok(BindOutcome::Held(held))
}

fn write_token(dir: &Path) -> io::Result<()> {
    let token = generate_token()?;
    let path = token_path_in(dir);
    std::fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn generate_token() -> io::Result<String> {
    let mut buf = [0u8; 32];
    fill_random(&mut buf)?;
    Ok(hex_encode(&buf))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn fill_random(buf: &mut [u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(buf)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!("BCryptGenRandom status {status}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_bind_should_return_already_held_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        let first = try_bind_in(tmp.path()).expect("first");
        assert!(matches!(first, BindOutcome::Held(_)));
        let second = try_bind_in(tmp.path()).expect("second");
        assert!(matches!(second, BindOutcome::AlreadyHeld));
    }

    #[test]
    fn spawn_params_from_launch_should_copy_cli_fields() {
        let opts = crate::cli::LaunchOptions {
            command: Some(vec!["true".into()]),
            working_directory: Some("/tmp".into()),
            title: Some("t".into()),
            hold: true,
            app_id: Some("baud-test".into()),
            ..crate::cli::LaunchOptions::default()
        };
        let p = SpawnParams::from_launch(&opts);
        assert_eq!(p.command.as_deref(), Some(["true".to_string()].as_slice()));
        assert_eq!(p.working_directory.as_deref(), Some("/tmp"));
        assert_eq!(p.title.as_deref(), Some("t"));
        assert!(p.hold);
        assert_eq!(p.app_id.as_deref(), Some("baud-test"));
    }
}
