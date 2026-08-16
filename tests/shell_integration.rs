//! OSC 133 A/B/C/D contra un PTY real con los scripts embebidos.

use std::io;
use std::path::PathBuf;
use std::process::Command;
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
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

fn find_after(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

fn marks_in_order(out: &[u8]) -> bool {
    let Some(a) = find_after(out, b"]133;A", 0) else {
        return false;
    };
    let Some(b) = find_after(out, b"]133;B", a) else {
        return false;
    };
    let Some(c) = find_after(out, b"]133;C", b) else {
        return false;
    };
    find_after(out, b"]133;D;0", c).is_some()
}

fn assert_marks_in_order(out: &[u8]) {
    let text = String::from_utf8_lossy(out);
    assert!(
        marks_in_order(out),
        "marcas A/B/C/D;0 fuera de orden en: {text:?}"
    );
}

fn which(bin: &str) -> Option<PathBuf> {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {bin}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/shell")
}

fn temp_home(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "baud-shell-int-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp home");
    dir
}

fn run_interactive_and_echo(cfg: ProcessConfig) -> Vec<u8> {
    let mut master = spawn_with(&cfg).expect("spawn");
    master.set_nonblocking().expect("nonblock");
    std::thread::sleep(Duration::from_millis(200));
    master.write_input(b"echo hi\n").expect("write");
    read_until(&mut master, marks_in_order, Duration::from_secs(8)).expect("read")
}

#[cfg(unix)]
#[test]
fn zsh_script_emite_marcas_abcd() {
    let Some(zsh) = which("zsh") else {
        eprintln!("skip: zsh no esta en el PATH");
        return;
    };
    let home = temp_home("zsh");
    let zdot = home.join("zdot");
    std::fs::create_dir_all(&zdot).expect("zdot");
    let script = assets_dir().join("baud.zsh");
    std::fs::write(
        zdot.join(".zshrc"),
        format!("PS1='$ '\nsource '{}'\n", script.display()),
    )
    .expect("zshrc");
    let out = run_interactive_and_echo(ProcessConfig {
        shell: zsh.to_string_lossy().into_owned(),
        args: Vec::new(),
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![
            ("HOME".into(), home.to_string_lossy().into_owned()),
            ("ZDOTDIR".into(), zdot.to_string_lossy().into_owned()),
        ],
        ..ProcessConfig::default()
    });
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn bash_script_emite_marcas_abcd() {
    let Some(bash) = which("bash") else {
        eprintln!("skip: bash no esta en el PATH");
        return;
    };
    let home = temp_home("bash");
    let rcfile = home.join("rcfile");
    let script = assets_dir().join("baud.bash");
    std::fs::write(
        &rcfile,
        format!("PS1='$ '\nsource '{}'\n", script.display()),
    )
    .expect("rcfile");
    let out = run_interactive_and_echo(ProcessConfig {
        shell: bash.to_string_lossy().into_owned(),
        args: vec!["--rcfile".into(), rcfile.to_string_lossy().into_owned()],
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        ..ProcessConfig::default()
    });
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}
