//! Control remoto de una instancia viva de Baud bajo Xvfb.
//!
//! Arranca `baud` con `remote_control = true` y `-e /bin/sh`, inyecta un echo
//! por el socket y afirma que la pantalla contiene el marcador. Cierra la
//! sesion con `exit` (no `ctrl+d`: ver el comentario junto a `send_text`).
//! con `BAUD_X11_E2E=1` (job `x11 e2e`) y `#[ignore]` para no correrlo en
//! `cargo test` sin display.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use baud::remote::server::Client;
use baud::remote::{Request, Response};

const APP_ID: &str = "baud-remote-e2e";

struct Harness {
    child: Child,
    runtime: PathBuf,
    config_home: PathBuf,
    state_home: PathBuf,
    stderr_log: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.runtime);
        let _ = fs::remove_dir_all(&self.config_home);
        let _ = fs::remove_dir_all(&self.state_home);
    }
}

impl Harness {
    fn diagnostics(&mut self) -> String {
        let stderr = fs::read_to_string(&self.stderr_log).unwrap_or_default();
        let mut logs = String::new();
        let log_dir = self.state_home.join("baud").join("logs");
        if let Ok(entries) = fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                if let Ok(text) = fs::read_to_string(entry.path()) {
                    logs.push_str(&text);
                }
            }
        }
        let runtime = fs::read_dir(self.runtime.join("baud"))
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let child = match self.child.try_wait() {
            Ok(Some(status)) => format!("exited {status}"),
            Ok(None) => "running".into(),
            Err(e) => format!("try_wait: {e}"),
        };
        format!("child={child}\nruntime files: [{runtime}]\nstderr:\n{stderr}\nlogs:\n{logs}")
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
    let state_home = base.join("state");
    fs::create_dir_all(runtime.join("baud")).expect("runtime dir");
    fs::create_dir_all(config_home.join("baud")).expect("config dir");
    fs::create_dir_all(state_home.join("baud").join("logs")).expect("state dir");
    fs::write(
        config_home.join("baud").join("config.toml"),
        "remote_control = true\n",
    )
    .expect("config.toml");
    let stderr_log = base.join("stderr.log");
    let stderr = fs::File::create(&stderr_log).expect("stderr log");

    // Runtime propio y escribible (en CI /run/user/<uid> a menudo no existe).
    // Sin WAYLAND para usar X11/Xvfb; DISPLAY lo hereda el job.
    let child = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(["--app-id", APP_ID, "-e", "/bin/sh"])
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_STATE_HOME", &state_home)
        .env("BAUD_SKIP_CONSENT_UI", "1")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("no se pudo lanzar el binario de baud");

    Harness {
        child,
        runtime,
        config_home,
        state_home,
        stderr_log,
    }
}

fn wait_endpoint(dir: &Path, timeout: Duration) -> Option<(String, String)> {
    let baud_dir = dir.join("baud");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(Some(pair)) = baud::remote::server::discover_in(&baud_dir) {
            return Some(pair);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// `resumed()` bloquea en wgpu antes de drenar UserEvent; en llvmpipe eso
/// puede durar más que el timeout de 5s del socket.
fn wait_event_loop(client: &mut Client, timeout: Duration) -> Result<Response, String> {
    let deadline = Instant::now() + timeout;
    let mut last = String::new();
    while Instant::now() < deadline {
        match client.call(Request::screen_text_default()) {
            Ok(resp) if resp.is_ok() => return Ok(resp),
            Ok(resp) => last = format!("{resp:?}"),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last)
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
            let diag = harness.diagnostics();
            let _ = harness.child.kill();
            panic!("el socket de control no apareció en 20s.\n{diag}");
        }
    };

    let mut client = Client::connect(&target, &token).expect("hello al socket de control");
    if let Err(last) = wait_event_loop(&mut client, Duration::from_secs(30)) {
        let diag = harness.diagnostics();
        panic!("el event loop no respondió en 30s. last={last}\n{diag}");
    }
    client
        .call(Request::send_text("echo BAUD_OK\n"))
        .expect("send_text");
    let waited = client
        .call(Request::wait_for("BAUD_OK", 15_000))
        .expect("wait_for");
    assert!(
        waited.is_ok(),
        "wait_for no vio BAUD_OK: {:?}\n{}",
        waited,
        harness.diagnostics()
    );
    let screen = client
        .call(Request::screen_text_default())
        .expect("screen_text");
    let text = screen.text_lines().unwrap_or_default().join("\n");
    assert!(
        text.contains("BAUD_OK"),
        "la pantalla no contiene BAUD_OK:\n{text}\n{}",
        harness.diagnostics()
    );
    // `ctrl+d` (0x04) no cierra /bin/sh de Debian/Ubuntu: el esclavo del PTY
    // queda sin ICANON tras cfmakeraw, y esa shell no trata 0x04 como EOF.
    // `exit` sí termina bash y dash; close_on_exit (lanzado con -e) cierra Baud.
    client
        .call(Request::send_text("exit\n"))
        .expect("send_text exit");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match harness.child.try_wait() {
            Ok(Some(_status)) => return,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => panic!("baud no termino tras exit\n{}", harness.diagnostics()),
            Err(e) => panic!("try_wait: {e}"),
        }
    }
}
