//! Suite de conformidad de sesión: spawn, I/O, resize, interrupt, shutdown.
//!
//! Comprobaciones de comportamiento (paridad de feeling), no de números de señal Unix.
//! Los casos Windows compilan bajo `cfg(windows)`.

use std::io;
use std::time::{Duration, Instant};

use baud::pty::{spawn_with, ProcessConfig, SessionBackend};

fn read_until(
    master: &mut baud::pty::Pty,
    pred: impl Fn(&[u8]) -> bool,
    timeout: Duration,
) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut scratch = [0u8; 4096];
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match master.read_output(&mut scratch) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&scratch[..n]);
                if pred(&out) {
                    return Ok(out);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

#[cfg(unix)]
fn unix_echo_cfg(script: &str) -> ProcessConfig {
    ProcessConfig {
        shell: "bash".into(),
        args: vec!["-c".into(), script.into()],
        ..ProcessConfig::default()
    }
}

#[cfg(unix)]
#[test]
fn conformance_spawn_echo_io() {
    let mut master = spawn_with(&unix_echo_cfg("echo CONFORM_OK")).expect("spawn");
    let out = read_until(
        &mut master,
        |b| b.windows(10).any(|w| w == b"CONFORM_OK"),
        Duration::from_secs(2),
    )
    .expect("read");
    assert!(
        out.windows(10).any(|w| w == b"CONFORM_OK"),
        "output: {:?}",
        String::from_utf8_lossy(&out)
    );
}

#[cfg(unix)]
#[test]
fn conformance_resize() {
    let mut master = spawn_with(&unix_echo_cfg("sleep 2")).expect("spawn");
    master.resize(40, 120).expect("resize");
    master.resize(24, 80).expect("resize again");
}

#[cfg(unix)]
#[test]
fn conformance_interrupt_stops_sleep() {
    // `exec` hace que sleep sea el líder de sesión; Ctrl+C (0x03) lo mata directo.
    let mut master = spawn_with(&unix_echo_cfg("exec sleep 30")).expect("spawn");
    master.set_nonblocking().expect("nonblock");
    std::thread::sleep(Duration::from_millis(100));
    master.interrupt().expect("interrupt");

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut scratch = [0u8; 256];
    let mut session_ended = false;
    while Instant::now() < deadline {
        match master.read_output(&mut scratch) {
            Ok(0) => {
                session_ended = true;
                break;
            }
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                // EIO / broken pipe tras la muerte del hijo = fin de sesión.
                session_ended = true;
                break;
            }
        }
    }
    assert!(
        session_ended,
        "se esperaba fin de sesión tras interrupt (EOF o error de pipe)"
    );
}

#[cfg(unix)]
#[test]
fn conformance_shutdown_graceful_then_drop() {
    let mut master = spawn_with(&unix_echo_cfg("sleep 30")).expect("spawn");
    assert!(master.shutdown_graceful());
    std::thread::sleep(Duration::from_millis(50));
    master.force_kill();
    drop(master);
}

#[cfg(unix)]
#[test]
fn conformance_double_shutdown_safe() {
    let mut master = spawn_with(&unix_echo_cfg("true")).expect("spawn");
    let _ = master.shutdown_graceful();
    let _ = master.shutdown_graceful();
    drop(master);
}

#[cfg(windows)]
fn windows_shell_cfg(args: Vec<String>) -> ProcessConfig {
    ProcessConfig {
        shell: "powershell.exe".into(),
        args,
        ..ProcessConfig::default()
    }
}

#[cfg(windows)]
#[test]
fn conformance_windows_spawn_echo() {
    let mut master = spawn_with(&windows_shell_cfg(vec![
        "-NoProfile".into(),
        "-Command".into(),
        "Write-Output 'CONFORM_OK'".into(),
    ]))
    .expect("spawn");
    let out = read_until(
        &mut master,
        |b| {
            let s = String::from_utf8_lossy(b);
            s.contains("CONFORM_OK")
        },
        Duration::from_secs(10),
    )
    .expect("read");
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("CONFORM_OK"), "output: {s:?}");
}

#[cfg(windows)]
#[test]
fn conformance_windows_echo_utf8() {
    let mut master = spawn_with(&windows_shell_cfg(vec![
        "-NoProfile".into(),
        "-Command".into(),
        "Write-Output 'CONFORM_áéíóúñ'".into(),
    ]))
    .expect("spawn");
    let out = read_until(
        &mut master,
        |b| {
            let s = String::from_utf8_lossy(b);
            s.contains("CONFORM_á")
        },
        Duration::from_secs(10),
    )
    .expect("read");
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("CONFORM_á"), "output: {s:?}");
}

#[cfg(windows)]
#[test]
fn conformance_windows_resize() {
    let mut master = spawn_with(&windows_shell_cfg(vec![
        "-NoProfile".into(),
        "-Command".into(),
        "Start-Sleep -Seconds 5".into(),
    ]))
    .expect("spawn");

    master.resize(30, 100).expect("resize 30x100");
    master.resize(24, 80).expect("resize 24x80");

    master.force_kill();
    drop(master);
}

#[cfg(windows)]
#[test]
fn conformance_windows_interrupt() {
    let mut master = spawn_with(&windows_shell_cfg(vec![
        "-NoProfile".into(),
        "-Command".into(),
        "Start-Sleep -Seconds 30".into(),
    ]))
    .expect("spawn");

    std::thread::sleep(Duration::from_millis(500));
    master.interrupt().expect("interrupt");

    // La interrupción debe terminar el proceso; forzamos kill
    // por si acaso y verificamos que no cuelga.
    std::thread::sleep(Duration::from_millis(500));
    master.force_kill();
    drop(master);
}

#[cfg(windows)]
#[test]
fn conformance_windows_shutdown() {
    let mut master = spawn_with(&windows_shell_cfg(vec![
        "-NoProfile".into(),
        "-Command".into(),
        "Start-Sleep -Seconds 30".into(),
    ]))
    .expect("spawn");

    assert!(master.shutdown_graceful());
    std::thread::sleep(Duration::from_millis(50));
    master.force_kill();
    drop(master);
}

#[cfg(windows)]
#[test]
fn conformance_windows_double_shutdown_safe() {
    let mut master = spawn_with(&windows_shell_cfg(vec![
        "-NoProfile".into(),
        "-Command".into(),
        "Write-Output 'ok'".into(),
    ]))
    .expect("spawn");

    let _ = master.shutdown_graceful();
    let _ = master.shutdown_graceful();
    drop(master);
}

#[cfg(windows)]
#[test]
fn conformance_windows_invalid_shell() {
    let cfg = ProcessConfig {
        shell: "shell_que_no_existe_xyz.exe".into(),
        ..ProcessConfig::default()
    };
    let result = spawn_with(&cfg);
    assert!(
        result.is_err(),
        "se esperaba error al spawn con shell inexistente"
    );
}
