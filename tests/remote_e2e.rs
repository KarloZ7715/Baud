//! Control remoto de una instancia viva de Baud bajo Xvfb.
//!
//! Arranca `baud` con `remote_control = true` y `-e sh`, inyecta un echo
//! por el socket y afirma que la pantalla contiene el marcador. Se ejecuta
//! con `BAUD_X11_E2E=1` (job `x11 e2e`) y `#[ignore]` para no correrlo en
//! `cargo test` sin display.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use baud::remote::server::{self, Client};
use baud::remote::Request;

const APP_ID: &str = "baud-remote-e2e";

struct Harness {
    child: Child,
    pid: u32,
    config_home: PathBuf,
    stderr_log: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Ok(dir) = server::runtime_dir() {
            let _ = fs::remove_file(dir.join(format!("{}.sock", self.pid)));
            let _ = fs::remove_file(dir.join(format!("{}.token", self.pid)));
        }
        let _ = fs::remove_dir_all(&self.config_home);
    }
}

impl Harness {
    fn stderr_tail(&self) -> String {
        fs::read_to_string(&self.stderr_log).unwrap_or_default()
    }
}

fn spawn_baud() -> Harness {
    let stamp = format!(
        "baud-remote-e2e-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let config_home = std::env::temp_dir().join(stamp);
    fs::create_dir_all(config_home.join("baud")).expect("config dir");
    fs::write(
        config_home.join("baud").join("config.toml"),
        "remote_control = true\n",
    )
    .expect("config.toml");
    let stderr_log = config_home.join("stderr.log");
    let stderr = fs::File::create(&stderr_log).expect("stderr log");

    // El compositor y D-Bus viven en el XDG_RUNTIME_DIR real; un runtime
    // aislado hace que la ventana ni llegue a crear el socket de control.
    let child = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(["--app-id", APP_ID, "-e", "sh"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env("BAUD_SKIP_CONSENT_UI", "1")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("no se pudo lanzar el binario de baud");
    let pid = child.id();

    Harness {
        child,
        pid,
        config_home,
        stderr_log,
    }
}

fn wait_endpoint(pid: u32, timeout: Duration) -> Option<(String, String)> {
    let dir = server::runtime_dir().ok()?;
    let sock = dir.join(format!("{pid}.sock"));
    let token_path = dir.join(format!("{pid}.token"));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if sock.exists() {
            if let Ok(token) = fs::read_to_string(&token_path) {
                let token = token.trim().to_string();
                if !token.is_empty() {
                    return Some((sock.to_string_lossy().into_owned(), token));
                }
            }
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
    let (target, token) = match wait_endpoint(harness.pid, Duration::from_secs(20)) {
        Some(pair) => pair,
        None => {
            let log = harness.stderr_tail();
            let _ = harness.child.kill();
            panic!("el socket de control no apareció en 20s. stderr:\n{log}");
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
