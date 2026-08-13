//! Control remoto de una instancia viva de Baud bajo Xvfb.
//!
//! Arranca `baud` con `remote_control = true` y `-e sh`, inyecta un echo
//! por el socket y afirma que la pantalla contiene el marcador. Se ejecuta
//! con `BAUD_X11_E2E=1` (job `x11 e2e`) y `#[ignore]` para no correrlo en
//! `cargo test` sin display.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use baud::remote::server::{self, Client};
use baud::remote::Request;

const APP_ID: &str = "baud-remote-e2e";

struct Harness {
    child: Child,
    runtime: PathBuf,
    config_home: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.runtime);
        let _ = fs::remove_dir_all(&self.config_home);
    }
}

fn spawn_baud() -> Harness {
    let stamp = format!(
        "baud-remote-e2e-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let base = std::env::temp_dir().join(stamp);
    let runtime = base.join("run");
    let config_home = base.join("config");
    fs::create_dir_all(runtime.join("baud")).expect("runtime dir");
    fs::create_dir_all(config_home.join("baud")).expect("config dir");
    fs::write(
        config_home.join("baud").join("config.toml"),
        "remote_control = true\n",
    )
    .expect("config.toml");

    let child = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(["--app-id", APP_ID, "-e", "sh"])
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("BAUD_SKIP_CONSENT_UI", "1")
        .spawn()
        .expect("no se pudo lanzar el binario de baud");

    Harness {
        child,
        runtime,
        config_home,
    }
}

fn wait_endpoint(dir: &std::path::Path, timeout: Duration) -> Option<(String, String)> {
    let baud_dir = dir.join("baud");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(Some(pair)) = server::discover_in(&baud_dir) {
            return Some(pair);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

#[test]
#[ignore]
fn control_socket_echo_y_cierre() {
    if std::env::var_os("BAUD_X11_E2E").is_none() {
        eprintln!("saltado: define BAUD_X11_E2E=1 con un servidor X disponible");
        return;
    }

    let mut harness = spawn_baud();
    let (target, token) = match wait_endpoint(&harness.runtime, Duration::from_secs(20)) {
        Some(pair) => pair,
        None => {
            let _ = harness.child.kill();
            panic!("el socket de control no apareció en 20s");
        }
    };

    let mut client = Client::connect(&target, &token).expect("hello al socket de control");
    client
        .call(Request::send_text("echo BAUD_OK\n"))
        .expect("send_text");
    let waited = client
        .call(Request::wait_for("BAUD_OK", 5_000))
        .expect("wait_for");
    assert!(waited.is_ok(), "wait_for no vio BAUD_OK: {:?}", waited);
    let screen = client
        .call(Request::screen_text_default())
        .expect("screen_text");
    let text = screen.text_lines().unwrap_or_default().join("\n");
    assert!(
        text.contains("BAUD_OK"),
        "la pantalla no contiene BAUD_OK:\n{text}"
    );
    client
        .call(Request::send_key("ctrl+d"))
        .expect("send_key ctrl+d");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match harness.child.try_wait() {
            Ok(Some(_status)) => return,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => panic!("baud no termino tras ctrl+d"),
            Err(e) => panic!("try_wait: {e}"),
        }
    }
}
