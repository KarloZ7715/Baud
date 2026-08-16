//! OSC 133 A/B/C/D contra un PTY real con los scripts embebidos.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use baud::pty::{spawn_with, ProcessConfig, SessionBackend, ShellIntegration};

#[cfg(unix)]
static ZDOTDIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        shell_integration: ShellIntegration::Off,
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
        shell_integration: ShellIntegration::Off,
        ..ProcessConfig::default()
    });
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}

/// Inyección automática: el spawn escribe scripts y fija ZDOTDIR; no se
/// prepara ZDOTDIR a mano.
#[cfg(unix)]
#[test]
fn zsh_auto_inyecta_marcas_abcd() {
    let Some(zsh) = which("zsh") else {
        eprintln!("skip: zsh no esta en el PATH");
        return;
    };
    let home = temp_home("zsh-auto");
    let _zdotdir_lock = ZDOTDIR_LOCK.lock().expect("zdotdir lock");
    let prev_zdot = std::env::var("ZDOTDIR").ok();
    unsafe {
        std::env::remove_var("ZDOTDIR");
    }
    let out = run_interactive_and_echo(ProcessConfig {
        shell: zsh.to_string_lossy().into_owned(),
        args: Vec::new(),
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        shell_integration: ShellIntegration::Auto,
        ..ProcessConfig::default()
    });
    match prev_zdot {
        Some(v) => unsafe {
            std::env::set_var("ZDOTDIR", v);
        },
        None => unsafe {
            std::env::remove_var("ZDOTDIR");
        },
    }
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn bash_auto_inyecta_marcas_abcd() {
    let Some(bash) = which("bash") else {
        eprintln!("skip: bash no esta en el PATH");
        return;
    };
    let home = temp_home("bash-auto");
    let out = run_interactive_and_echo(ProcessConfig {
        shell: bash.to_string_lossy().into_owned(),
        args: Vec::new(),
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        shell_integration: ShellIntegration::Auto,
        ..ProcessConfig::default()
    });
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}

fn spawn_ready(cfg: ProcessConfig) -> baud::pty::Pty {
    let mut master = spawn_with(&cfg).expect("spawn");
    master.set_nonblocking().expect("nonblock");
    std::thread::sleep(Duration::from_millis(200));
    master
}

fn write_line(master: &mut baud::pty::Pty, line: &str) {
    master.write_input(line.as_bytes()).expect("write");
    if !line.ends_with('\n') {
        master.write_input(b"\n").expect("newline");
    }
}

#[cfg(unix)]
#[test]
fn bash_false_emite_d_1() {
    let Some(bash) = which("bash") else {
        eprintln!("skip: bash no esta en el PATH");
        return;
    };
    let home = temp_home("bash-d1");
    let rcfile = home.join("rcfile");
    let script = assets_dir().join("baud.bash");
    std::fs::write(
        &rcfile,
        format!(
            "PS1='$ '\nPROMPT_COMMAND='printf \"USERST:%s\\n\" \"$?\"'\nsource '{}'\n",
            script.display()
        ),
    )
    .expect("rcfile");
    let mut master = spawn_ready(ProcessConfig {
        shell: bash.to_string_lossy().into_owned(),
        args: vec!["--rcfile".into(), rcfile.to_string_lossy().into_owned()],
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        shell_integration: ShellIntegration::Off,
        ..ProcessConfig::default()
    });
    write_line(&mut master, "false");
    let out = read_until(
        &mut master,
        |b| find_after(b, b"]133;D;1", 0).is_some() && find_after(b, b"USERST:1", 0).is_some(),
        Duration::from_secs(8),
    )
    .expect("read");
    let text = String::from_utf8_lossy(&out);
    assert!(
        find_after(&out, b"]133;D;1", 0).is_some(),
        "falta D;1 en: {text:?}"
    );
    assert!(
        find_after(&out, b"USERST:1", 0).is_some(),
        "el PROMPT_COMMAND del usuario no vio $?=1 en: {text:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn bash_ps1_dinamico_conserva_b() {
    let Some(bash) = which("bash") else {
        eprintln!("skip: bash no esta en el PATH");
        return;
    };
    let home = temp_home("bash-dynps1");
    let rcfile = home.join("rcfile");
    let script = assets_dir().join("baud.bash");
    std::fs::write(
        &rcfile,
        format!(
            "PROMPT_COMMAND='PS1=\"x \"'\nsource '{}'\n",
            script.display()
        ),
    )
    .expect("rcfile");
    let out = run_interactive_and_echo(ProcessConfig {
        shell: bash.to_string_lossy().into_owned(),
        args: vec!["--rcfile".into(), rcfile.to_string_lossy().into_owned()],
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        shell_integration: ShellIntegration::Off,
        ..ProcessConfig::default()
    });
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn bash_prompt_command_array_no_emite_c_antes_de_b() {
    let Some(bash) = which("bash") else {
        eprintln!("skip: bash no esta en el PATH");
        return;
    };
    let ver = Command::new(&bash)
        .arg("-c")
        .arg("printf '%s' \"$BASH_VERSINFO\"")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    if ver < 5 {
        eprintln!("skip: hace falta bash 5+ para PROMPT_COMMAND array");
        return;
    }
    let home = temp_home("bash-pcarr");
    let rcfile = home.join("rcfile");
    let script = assets_dir().join("baud.bash");
    std::fs::write(
        &rcfile,
        format!(
            "PS1='$ '\nPROMPT_COMMAND=('true' 'true')\nsource '{}'\n",
            script.display()
        ),
    )
    .expect("rcfile");
    let mut master = spawn_ready(ProcessConfig {
        shell: bash.to_string_lossy().into_owned(),
        args: vec!["--rcfile".into(), rcfile.to_string_lossy().into_owned()],
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        shell_integration: ShellIntegration::Off,
        ..ProcessConfig::default()
    });
    let first = read_until(
        &mut master,
        |b| find_after(b, b"]133;A", 0).is_some() && find_after(b, b"]133;B", 0).is_some(),
        Duration::from_secs(8),
    )
    .expect("read prompt");
    let text = String::from_utf8_lossy(&first);
    let b = find_after(&first, b"]133;B", 0).expect("B");
    if let Some(c) = find_after(&first, b"]133;C", 0) {
        assert!(
            c > b,
            "C del array PROMPT_COMMAND salio antes de B: {text:?}"
        );
    }
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn zsh_ps1_dinamico_conserva_b() {
    let Some(zsh) = which("zsh") else {
        eprintln!("skip: zsh no esta en el PATH");
        return;
    };
    let home = temp_home("zsh-dynps1");
    let zdot = home.join("zdot");
    std::fs::create_dir_all(&zdot).expect("zdot");
    let script = assets_dir().join("baud.zsh");
    std::fs::write(
        zdot.join(".zshrc"),
        format!("precmd() {{ PS1='x ' }}\nsource '{}'\n", script.display()),
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
        shell_integration: ShellIntegration::Off,
        ..ProcessConfig::default()
    });
    assert_marks_in_order(&out);
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn zsh_auto_fuente_zshenv_y_restaura_zdotdir() {
    let Some(zsh) = which("zsh") else {
        eprintln!("skip: zsh no esta en el PATH");
        return;
    };
    let home = temp_home("zsh-env");
    std::fs::write(home.join(".zshenv"), "print -r -- ZSHENV_OK\n").expect("zshenv");
    std::fs::write(
        home.join(".zshrc"),
        "print -r -- \"ZDOT=${ZDOTDIR-UNSET}\"\nPS1='$ '\n",
    )
    .expect("zshrc");
    let _zdotdir_lock = ZDOTDIR_LOCK.lock().expect("zdotdir lock");
    let prev_zdot = std::env::var("ZDOTDIR").ok();
    unsafe {
        std::env::remove_var("ZDOTDIR");
    }
    let mut master = spawn_ready(ProcessConfig {
        shell: zsh.to_string_lossy().into_owned(),
        args: Vec::new(),
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        shell_integration: ShellIntegration::Auto,
        ..ProcessConfig::default()
    });
    let out = read_until(
        &mut master,
        |b| find_after(b, b"ZSHENV_OK", 0).is_some() && find_after(b, b"ZDOT=", 0).is_some(),
        Duration::from_secs(8),
    )
    .expect("read");
    match prev_zdot {
        Some(v) => unsafe {
            std::env::set_var("ZDOTDIR", v);
        },
        None => unsafe {
            std::env::remove_var("ZDOTDIR");
        },
    }
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("ZSHENV_OK"), "no se fuente .zshenv: {text:?}");
    assert!(
        !text.contains("shell-integration"),
        "ZDOTDIR siguio en el dir de runtime: {text:?}"
    );
    assert!(
        text.contains("ZDOT=UNSET") || text.contains("ZDOT=\n") || text.contains("ZDOT=\r"),
        "ZDOTDIR no se restauro al valor del usuario: {text:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn zsh_login_fuente_zprofile() {
    let Some(zsh) = which("zsh") else {
        eprintln!("skip: zsh no esta en el PATH");
        return;
    };
    let home = temp_home("zsh-login");
    std::fs::write(home.join(".zprofile"), "print -r -- ZPROFILE_OK\n").expect("zprofile");
    std::fs::write(home.join(".zshrc"), "PS1='$ '\n").expect("zshrc");
    let _zdotdir_lock = ZDOTDIR_LOCK.lock().expect("zdotdir lock");
    let prev_zdot = std::env::var("ZDOTDIR").ok();
    unsafe {
        std::env::remove_var("ZDOTDIR");
    }
    let mut master = spawn_ready(ProcessConfig {
        shell: zsh.to_string_lossy().into_owned(),
        args: Vec::new(),
        working_directory: Some(home.to_string_lossy().into_owned()),
        env: vec![("HOME".into(), home.to_string_lossy().into_owned())],
        login_shell: true,
        shell_integration: ShellIntegration::Auto,
        ..ProcessConfig::default()
    });
    let out = read_until(
        &mut master,
        |b| find_after(b, b"ZPROFILE_OK", 0).is_some(),
        Duration::from_secs(8),
    )
    .expect("read");
    match prev_zdot {
        Some(v) => unsafe {
            std::env::set_var("ZDOTDIR", v);
        },
        None => unsafe {
            std::env::remove_var("ZDOTDIR");
        },
    }
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("ZPROFILE_OK"),
        "login no fuente .zprofile: {text:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}
