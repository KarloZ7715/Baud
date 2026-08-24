//! Cliente del socket de spawn: pide una tab y sale.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;

use super::{socket_path_in, token_path_in, SpawnParams};
use crate::cli::LaunchOptions;

pub fn connect_and_spawn(params: &SpawnParams) -> io::Result<()> {
    connect_and_spawn_in(&crate::remote::server::runtime_dir()?, params)
}

pub fn connect_and_spawn_in(dir: &Path, params: &SpawnParams) -> io::Result<()> {
    let token = std::fs::read_to_string(token_path_in(dir))?;
    let target = {
        #[cfg(unix)]
        {
            socket_path_in(dir).to_string_lossy().into_owned()
        }
        #[cfg(windows)]
        {
            std::fs::read_to_string(socket_path_in(dir))?
                .trim()
                .to_string()
        }
    };
    let mut client = crate::remote::server::Client::connect(&target, token.trim())?;
    let body = json!({
        "command": params.command,
        "working_directory": params.working_directory,
        "title": params.title,
        "hold": params.hold,
        "app_id": params.app_id,
    });
    let resp = client.call(crate::remote::Request {
        id: 1,
        method: "new_tab".into(),
        params: body,
    })?;
    if resp.is_ok() {
        Ok(())
    } else {
        Err(io::Error::other("spawn new_tab failed"))
    }
}

/// Habla con un daemon ya bindeado. No arranca `--server`.
pub fn run_as_client_in(dir: &Path, opts: &LaunchOptions) -> i32 {
    let params = SpawnParams::from_launch(opts);
    if connect_and_spawn_in(dir, &params).is_ok() {
        0
    } else {
        1
    }
}

/// Cliente corto: conecta o arranca el daemon y pide una tab.
pub fn run_as_client(opts: &LaunchOptions) -> i32 {
    let dir = match crate::remote::server::runtime_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let params = SpawnParams::from_launch(opts);
    if socket_connectable(&dir) && connect_and_spawn_in(&dir, &params).is_ok() {
        return 0;
    }
    if let Err(e) = spawn_daemon_detached(opts) {
        eprintln!("Error: failed to start baud --server: {e}");
        return 1;
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if connect_and_spawn_in(&dir, &params).is_ok() {
            return 0;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("Error: could not connect to the Baud daemon");
    1
}

fn socket_connectable(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(socket_path_in(dir)).is_ok()
    }
    #[cfg(windows)]
    {
        let Ok(name) = std::fs::read_to_string(socket_path_in(dir)) else {
            return false;
        };
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name.trim())
            .is_ok()
    }
}

pub fn spawn_daemon_detached(opts: &LaunchOptions) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--server");
    if let Some(path) = &opts.config_path {
        cmd.arg("--config").arg(path);
    }
    for pair in &opts.overrides {
        cmd.arg("-o").arg(pair);
    }
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env("XDG_RUNTIME_DIR", dir);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid solo desprende al hijo de la sesion de terminal del
        // cliente; no toca memoria compartida ni fds extra.
        unsafe {
            cmd.pre_exec(|| nix::unistd::setsid().map(|_| ()).map_err(io::Error::from));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    cmd.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn::server::serve_with_handler;
    use crate::spawn::{try_bind_in, BindOutcome};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn run_as_client_in_should_talk_to_existing_daemon() {
        let tmp = tempfile::tempdir().unwrap();
        let BindOutcome::Held(listener) = try_bind_in(tmp.path()).unwrap() else {
            panic!("bind");
        };
        let _h = serve_with_handler(
            listener,
            Arc::new(|_params, tx| {
                let _ = tx.send(crate::remote::Response::ok(1, json!({})));
            }),
        );
        let code = run_as_client_in(tmp.path(), &LaunchOptions::default());
        assert_eq!(code, 0);
    }
}
