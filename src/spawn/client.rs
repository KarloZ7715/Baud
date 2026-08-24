//! Cliente del socket de spawn: pide una tab y sale.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::io;
use std::path::Path;

use serde_json::json;

use super::{socket_path_in, token_path_in, SpawnParams};

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
