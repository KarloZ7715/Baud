//! Cliente de spawn contra un daemon bajo Xvfb.
//!
//! Tres escenarios: dos tabs en un daemon, carrera de dos clientes sin
//! servidor previo, y linger a cero ventanas. Se corre con `BAUD_X11_E2E=1`
//! (job `x11 e2e`) y `#[ignore]` para no ejecutarlo en `cargo test` sin display.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use baud::spawn::client::connect_and_spawn_in;
use baud::spawn::{socket_path_in, SpawnParams};

const APP_ID: &str = "baud-spawn-e2e";

struct Harness {
    child: Option<Child>,
    runtime: PathBuf,
    config_home: PathBuf,
    state_home: PathBuf,
    stderr_log: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.runtime);
        let _ = fs::remove_dir_all(&self.config_home);
        let _ = fs::remove_dir_all(&self.state_home);
    }
}

impl Harness {
    fn baud_dir(&self) -> PathBuf {
        self.runtime.join("baud")
    }

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
        let runtime = fs::read_dir(self.baud_dir())
            .map(|it| {
                it.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let child = match self.child.as_mut() {
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => format!("exited {status}"),
                Ok(None) => "running".into(),
                Err(e) => format!("try_wait: {e}"),
            },
            None => "none".into(),
        };
        format!("child={child}\nruntime files: [{runtime}]\nstderr:\n{stderr}\nlogs:\n{logs}")
    }

    fn log_text(&self) -> String {
        let log_dir = self.state_home.join("baud").join("logs");
        let mut logs = String::new();
        if let Ok(entries) = fs::read_dir(&log_dir) {
            for entry in entries.flatten() {
                if let Ok(text) = fs::read_to_string(entry.path()) {
                    logs.push_str(&text);
                }
            }
        }
        logs.push_str(&fs::read_to_string(&self.stderr_log).unwrap_or_default());
        logs
    }
}

fn skip_without_display() -> bool {
    if std::env::var_os("BAUD_X11_E2E").is_none() {
        eprintln!("saltado: define BAUD_X11_E2E=1 con un servidor X disponible");
        true
    } else {
        false
    }
}

fn make_dirs() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let stamp = format!(
        "baud-spawn-e2e-{}-{}",
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
    (runtime, config_home, state_home, base)
}

fn spawn_server() -> Harness {
    let (runtime, config_home, state_home, base) = make_dirs();
    let stderr_log = base.join("stderr.log");
    let stderr = fs::File::create(&stderr_log).expect("stderr log");
    let child = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(["--server", "--app-id", APP_ID])
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_STATE_HOME", &state_home)
        .env("BAUD_SKIP_CONSENT_UI", "1")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("no se pudo lanzar baud --server");
    Harness {
        child: Some(child),
        runtime,
        config_home,
        state_home,
        stderr_log,
    }
}

fn wait_spawn_ready(dir: &Path, timeout: Duration) -> bool {
    let baud_dir = dir.join("baud");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let sock = socket_path_in(&baud_dir);
        if sock.exists()
            && std::os::unix::net::UnixStream::connect(&sock).is_ok()
            && baud_dir.join("spawn.token").exists()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn spawn_params_shell() -> SpawnParams {
    SpawnParams {
        command: Some(vec!["/bin/sh".into()]),
        ..SpawnParams::default()
    }
}

fn kill_spawn_daemon(dir: &Path) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let Ok(token) = fs::read_to_string(dir.join("spawn.token")) else {
        return;
    };
    let Ok(mut stream) = UnixStream::connect(socket_path_in(dir)) else {
        return;
    };
    let hello = serde_json::json!({
        "id": 0,
        "method": "hello",
        "params": { "token": token.trim() }
    });
    if writeln!(stream, "{hello}").is_err() {
        return;
    }
    let _ = stream.flush();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).ok().unwrap_or(0) == 0 {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return;
    };
    let Some(pid) = v
        .pointer("/ok/pid")
        .and_then(|p| p.as_u64())
        .map(|p| p as i32)
    else {
        return;
    };
    if pid > 1 {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

fn spawn_params_true() -> SpawnParams {
    SpawnParams {
        command: Some(vec!["true".into()]),
        ..SpawnParams::default()
    }
}

#[test]
#[ignore]
fn dos_clientes_un_daemon_segunda_peticion_es_tab() {
    if skip_without_display() {
        return;
    }
    let mut harness = spawn_server();
    if !wait_spawn_ready(&harness.runtime, Duration::from_secs(20)) {
        let diag = harness.diagnostics();
        panic!("el socket de spawn no apareció en 20s.\n{diag}");
    }
    let dir = harness.baud_dir();
    connect_and_spawn_in(&dir, &spawn_params_shell()).expect("primer new_tab");
    connect_and_spawn_in(&dir, &spawn_params_shell()).expect("segundo new_tab");
    match harness.child.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => {}
        other => panic!(
            "el daemon no sigue vivo: {other:?}\n{}",
            harness.diagnostics()
        ),
    }
}

#[test]
#[ignore]
fn carrera_de_dos_clientes_sin_daemon() {
    if skip_without_display() {
        return;
    }
    let (runtime, config_home, state_home, base) = make_dirs();
    let spawn_client = |stderr_path: PathBuf| {
        let stderr = fs::File::create(&stderr_path).expect("stderr");
        Command::new(env!("CARGO_BIN_EXE_baud"))
            .args(["--app-id", APP_ID, "-e", "true"])
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("XDG_CONFIG_HOME", &config_home)
            .env("XDG_STATE_HOME", &state_home)
            .env("BAUD_SKIP_CONSENT_UI", "1")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("cliente baud")
    };
    let log_a = base.join("client-a.log");
    let log_b = base.join("client-b.log");
    let mut a = spawn_client(log_a.clone());
    let mut b = spawn_client(log_b.clone());
    let deadline = Instant::now() + Duration::from_secs(60);
    let wait = |child: &mut Child, name: &str, log: &Path| loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(
                    status.success(),
                    "{name} salió {status}\n{}",
                    fs::read_to_string(log).unwrap_or_default()
                );
                return;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => panic!(
                "{name} no terminó en 60s\n{}",
                fs::read_to_string(log).unwrap_or_default()
            ),
            Err(e) => panic!("try_wait {name}: {e}"),
        }
    };
    wait(&mut a, "cliente A", &log_a);
    wait(&mut b, "cliente B", &log_b);
    let socks: Vec<_> = fs::read_dir(runtime.join("baud"))
        .expect("runtime")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "spawn.sock")
        .collect();
    assert_eq!(socks.len(), 1, "debe haber un solo spawn.sock");
    kill_spawn_daemon(&runtime.join("baud"));
    let _ = a.kill();
    let _ = b.kill();
    let _ = fs::remove_dir_all(&runtime);
    let _ = fs::remove_dir_all(&config_home);
    let _ = fs::remove_dir_all(&state_home);
}

#[test]
#[ignore]
fn linger_reabre_sin_reescanear_fuentes() {
    if skip_without_display() {
        return;
    }
    let mut harness = spawn_server();
    if !wait_spawn_ready(&harness.runtime, Duration::from_secs(20)) {
        let diag = harness.diagnostics();
        panic!("el socket de spawn no apareció en 20s.\n{diag}");
    }
    let dir = harness.baud_dir();
    connect_and_spawn_in(&dir, &spawn_params_true()).expect("primer new_tab");
    std::thread::sleep(Duration::from_secs(1));
    match harness.child.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => {}
        other => panic!(
            "el daemon murió al cerrar la tab: {other:?}\n{}",
            harness.diagnostics()
        ),
    }
    let fonts_before = harness.log_text().matches("load_system_fonts").count();
    connect_and_spawn_in(&dir, &spawn_params_shell()).expect("segundo new_tab tras linger");
    let fonts_after = harness.log_text().matches("load_system_fonts").count();
    assert_eq!(
        fonts_after,
        fonts_before,
        "el segundo new_tab no debe reescanear fuentes\n{}",
        harness.diagnostics()
    );
}
