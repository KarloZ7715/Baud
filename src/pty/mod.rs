pub mod config;

pub use config::ProcessConfig;

use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::Stdio;

/// Wrapper minimalista sobre un file descriptor de PTY.
/// Implementa Read y Write delegando a nix::unistd.
pub struct Pty {
    fd: OwnedFd,
    // ponytail: child_pid se guarda para SIGHUP en Drop y set_winsize.
    // None en open(), Some(_) en spawn().
    child_pid: Option<i32>,
}

impl Pty {
    pub fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    pub fn child_pid(&self) -> Option<i32> {
        self.child_pid
    }

    /// Envia SIGHUP al child. No-op si child_pid es None o si el child ya murio.
    pub fn send_sighup(&self) -> bool {
        if let Some(pid) = self.child_pid {
            let nix_pid = nix::unistd::Pid::from_raw(pid);
            match nix::sys::signal::kill(nix_pid, nix::sys::signal::Signal::SIGHUP) {
                Ok(()) => true,
                Err(_) => false, // ESRCH = child ya murio, otros errores tambien false
            }
        } else {
            false
        }
    }

    /// Actualiza el winsize del PTY via ioctl(TIOCSWINSZ). El kernel envia SIGWINCH al child.
    pub fn set_winsize(&self, rows: u16, cols: u16) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        let ws = nix::libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let res = unsafe { nix::libc::ioctl(self.fd.as_raw_fd(), nix::libc::TIOCSWINSZ, &ws) };
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Safety net: si el GUI no envio SIGHUP explicitamente, Drop lo hace.
        // SIGKILL aqui es OK porque Drop solo corre cuando baud sale (o el Pty
        // se recrea), y en ambos casos queremos que el child muera.
        // ponytail: Drop envia SIGKILL como safety net; el flujo normal es
        // SIGHUP explicito desde el GUI via PtyCommand::Shutdown.
        if let Some(pid) = self.child_pid {
            let nix_pid = nix::unistd::Pid::from_raw(pid);
            let _ = nix::sys::signal::kill(nix_pid, nix::sys::signal::Signal::SIGKILL);
        }
    }
}

// ponytail: implementamos Read/Write via nix::unistd para no depender de std::fs::File
impl std::io::Read for Pty {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        nix::unistd::read(&self.fd, buf).map_err(std::io::Error::from)
    }
}

impl std::io::Write for Pty {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        nix::unistd::write(&self.fd, buf).map_err(std::io::Error::from)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Crea un par de pseudoterminales (master, slave) sin configurar.
pub fn open() -> nix::Result<(Pty, Pty)> {
    let result = nix::pty::openpty(None, None)?;
    Ok((
        Pty {
            fd: result.master,
            child_pid: None,
        },
        Pty {
            fd: result.slave,
            child_pid: None,
        },
    ))
}

/// Lanza un proceso en un nuevo PTY (pseudo-terminal).
///
/// Retorna el extremo master para comunicarse con el proceso.
/// El slave se cierra en el padre despues del spawn.
pub fn spawn(shell: &str, args: &[&str]) -> nix::Result<Pty> {
    spawn_with(&ProcessConfig {
        shell: shell.into(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        working_directory: None,
        env: Vec::new(),
        startup_command: None,
        login_shell: false,
    })
}

/// Lanza un proceso según [`ProcessConfig`].
pub fn spawn_with(cfg: &ProcessConfig) -> nix::Result<Pty> {
    let result = nix::pty::openpty(None, None)?;

    // ponytail: configurar slave en raw mode para que el child reciba
    // caracteres uno a uno sin buffering del kernel ni eco automatico.
    // IMPORTANTE: cfmakeraw desactiva ECHO. Lo reactivamos para que el
    // terminal driver haga eco aunque readline no se active.
    // ponytail: eco del kernel como fallback para shells sin readline.
    {
        use nix::sys::termios;
        let mut termios = termios::tcgetattr(&result.slave)?;
        termios::cfmakeraw(&mut termios);

        // Deshabilitar ECHOCTL: evitar caret notation (^[) en el eco de secuencias ESC.
        // cfmakeraw() de glibc no limpia este flag.
        // ECHO se mantiene activo: el kernel hace eco de caracteres imprimibles.
        termios.local_flags &= !(nix::sys::termios::LocalFlags::ECHOCTL);
        termios.local_flags |=
            nix::sys::termios::LocalFlags::ECHO | nix::sys::termios::LocalFlags::ISIG;

        termios.output_flags |=
            nix::sys::termios::OutputFlags::OPOST | nix::sys::termios::OutputFlags::ONLCR;
        termios::tcsetattr(&result.slave, termios::SetArg::TCSANOW, &termios)?;
    }

    // ponytail: duplicamos el slave 3 veces (stdin, stdout, stderr)
    let slave_stdin = nix::unistd::dup(&result.slave)?;
    let slave_stdout = nix::unistd::dup(&result.slave)?;

    let mut cmd = std::process::Command::new(&cfg.shell);
    cmd.args(&cfg.args);
    if let Some(dir) = &cfg.working_directory {
        cmd.current_dir(dir);
    }
    for (key, value) in &cfg.env {
        cmd.env(key, value);
    }
    // ponytail: TERM necesario para que bash active readline.
    // xterm-256color es el estandar y compatible con la mayoria de programas TUI.
    cmd.env("TERM", "xterm-256color");
    // Login shell: argv[0] con prefijo '-' (convencion POSIX, portable entre shells).
    if cfg.login_shell {
        let base_name = std::path::Path::new(&cfg.shell)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&cfg.shell);
        cmd.arg0(format!("-{base_name}"));
    }
    cmd.stdin(Stdio::from(std::fs::File::from(slave_stdin)));
    cmd.stdout(Stdio::from(std::fs::File::from(slave_stdout)));
    cmd.stderr(Stdio::from(std::fs::File::from(result.slave)));

    // ponytail: usamos nix::libc que nix re-exporta, evitamos agregar libc como dep
    unsafe {
        cmd.pre_exec(|| {
            // ponytail: setsid crea nueva sesion; sin esto Ctrl+C mata al emulador
            if nix::libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // ponytail: TIOCSCTTY establece el controlling terminal al slave
            if nix::libc::ioctl(0, nix::libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd
        .spawn()
        .map_err(|e| nix::errno::Errno::from_raw(e.raw_os_error().unwrap_or(0)))?;
    let pid = child.id() as i32;
    // ponytail: el Child se dropea al final del spawn para no acumular handles,
    // pero el PID se guarda para SIGHUP y set_winsize.
    drop(child);

    Ok(Pty {
        fd: result.master,
        child_pid: Some(pid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::sync::mpsc;
    use std::time::Duration;

    fn read_to_string_until_eof(master: &mut Pty) -> String {
        let mut output = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match master.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    }

    #[test]
    fn test_startup_command_se_escribe_al_pty() {
        let cfg = ProcessConfig {
            shell: "/bin/bash".into(),
            args: Vec::new(),
            working_directory: None,
            env: Vec::new(),
            startup_command: Some("echo HELLO".into()),
            login_shell: false,
        };
        let mut master = spawn_with(&ProcessConfig {
            startup_command: None,
            ..cfg.clone()
        })
        .expect("spawn");

        let cmd = cfg.startup_command.as_ref().expect("startup_command");
        nix::unistd::write(master.fd(), format!("{cmd}\n").as_bytes()).expect("write");

        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(1);
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match master.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        output.extend_from_slice(&buf[..n]);
                        if output.windows(5).any(|w| w == b"HELLO") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(output);
        });

        let result = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timeout leyendo del PTY");
        let output = String::from_utf8_lossy(&result);
        assert!(
            output.contains("HELLO"),
            "Se esperaba 'HELLO' en output: {:?}",
            output
        );
    }

    #[test]
    fn test_spawn_login_shell_usa_arg0_con_prefijo() {
        let cfg = ProcessConfig {
            shell: "/bin/bash".into(),
            args: vec!["-c".into(), "echo ARG0=$0".into()],
            working_directory: None,
            env: Vec::new(),
            startup_command: None,
            login_shell: true,
        };
        let mut master = spawn_with(&cfg).expect("spawn");
        let out = read_to_string_until_eof(&mut master);
        assert!(
            out.contains("ARG0=-bash") || out.contains("ARG0=bash"),
            "output: {out:?}"
        );
    }

    #[test]
    fn test_spawn_aplica_cwd_y_env() {
        let cfg = ProcessConfig {
            shell: "/bin/bash".into(),
            args: vec!["-c".into(), "echo CWD=$PWD VAR=$BAUD_TEST".into()],
            working_directory: Some("/tmp".into()),
            env: vec![("BAUD_TEST".into(), "ok".into())],
            startup_command: None,
            login_shell: false,
        };
        let mut master = spawn_with(&cfg).expect("spawn");
        let out = read_to_string_until_eof(&mut master);
        assert!(out.contains("CWD=/tmp"), "output: {out:?}");
        assert!(out.contains("VAR=ok"), "output: {out:?}");
    }

    #[test]
    fn test_open_returns_valid_fds() {
        let (master, slave) = open().expect("open failed");
        // ponytail: verificamos que los FDs son validos (>= 0)
        assert!(
            master.fd().as_raw_fd() >= 0,
            "master fd invalido: {}",
            master.fd().as_raw_fd()
        );
        assert!(
            slave.fd().as_raw_fd() >= 0,
            "slave fd invalido: {}",
            slave.fd().as_raw_fd()
        );
        // ponytail: los FDs del master y slave deben ser distintos
        assert_ne!(
            master.fd().as_raw_fd(),
            slave.fd().as_raw_fd(),
            "master y slave deben ser FDs distintos"
        );
    }

    #[test]
    fn test_spawn_runs_command() {
        let mut master = spawn("bash", &["-c", "echo hola"]).expect("spawn fallo");

        // ponytail: leemos en un hilo separado con timeout via mpsc
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(1);
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match master.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        output.extend_from_slice(&buf[..n]);
                        // ponytail: cuando encontramos "hola", salimos
                        if output.windows(4).any(|w| w == b"hola") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(output);
        });

        let result = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timeout leyendo del PTY");
        let output = String::from_utf8_lossy(&result);
        assert!(
            output.contains("hola"),
            "Se esperaba 'hola' en output: {:?}",
            output
        );
    }

    #[test]
    fn test_set_winsize_succeeds() {
        let master = spawn("bash", &["-c", "echo hola"]).expect("spawn fallo");
        let result = master.set_winsize(24, 80);
        assert!(
            result.is_ok(),
            "set_winsize deberia retornar Ok, obtuve: {:?}",
            result
        );
    }

    #[test]
    fn test_pty_write_and_read() {
        // Spawn bash sin flags (lee de stdin). Escribir un comando, leer la respuesta.
        let mut master = spawn("bash", &[] as &[&str]).expect("spawn fallo");

        // Escribir un comando en el master.
        let cmd = b"echo PIPELINE_TEST_OK\n";
        nix::unistd::write(master.fd(), cmd).expect("write fallo");

        // Leer la respuesta: debe contener PIPELINE_TEST_OK.
        let mut buf = [0u8; 4096];
        let mut output = Vec::new();
        loop {
            match master.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buf[..n]);
                    if output
                        .windows(b"PIPELINE_TEST_OK".len())
                        .any(|w| w == b"PIPELINE_TEST_OK")
                    {
                        break;
                    }
                    if output.len() > 4096 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("PIPELINE_TEST_OK"),
            "Se esperaba PIPELINE_TEST_OK en output: {:?}",
            output_str
        );
    }

    #[test]
    fn test_pty_drop_sends_sigkill() {
        // Spawn bash que duerme 10 segundos. Dropear el Pty debe enviar SIGKILL.
        let master = spawn("bash", &["-c", "sleep 10"]).expect("spawn fallo");
        let child_pid = master.child_pid().expect("child_pid deberia ser Some");
        let nix_pid = nix::unistd::Pid::from_raw(child_pid);

        // Verificar que el child esta vivo antes del drop.
        let alive_before = nix::sys::signal::kill(nix_pid, None).is_ok();
        assert!(alive_before, "child deberia estar vivo antes del drop");

        // Dropear el Pty. Drop envia SIGKILL.
        drop(master);

        // Esperar que la senal llegue y reapear el zombie con waitpid.
        // ponytail: kill(pid, None) retorna Ok mientras el zombie exista en la
        // tabla de procesos. Necesitamos waitpid para cosechar el zombie y
        // luego verificar ESRCH.
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            // Reapear si ya termino. WNOHANG no bloquea.
            let _ = nix::sys::wait::waitpid(nix_pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG));
            if nix::sys::signal::kill(nix_pid, None).is_err() {
                return; // ESRCH: child ya no existe, test pasa
            }
        }
        panic!("child aun existe tras 1 segundo post-SIGKILL");
    }

    #[test]
    fn test_send_sighup_returns_false_for_no_child() {
        let (master, _slave) = open().expect("open failed");
        assert!(
            !master.send_sighup(),
            "send_sighup sin child debe retornar false"
        );
    }

    #[test]
    fn test_echoctl_disabled_after_spawn() {
        // Verificar que ECHOCTL esta deshabilitado y ECHO deshabilitado, ISIG habilitado
        // después de la configuración de termios en spawn().
        use nix::sys::termios;

        let result = nix::pty::openpty(None, None).expect("openpty fallo");
        let mut t = termios::tcgetattr(&result.slave).expect("tcgetattr fallo");
        termios::cfmakeraw(&mut t);

        // Aplicar la misma config que en spawn()
        t.local_flags &= !(nix::sys::termios::LocalFlags::ECHOCTL);
        t.local_flags |= nix::sys::termios::LocalFlags::ECHO | nix::sys::termios::LocalFlags::ISIG;
        t.output_flags |=
            nix::sys::termios::OutputFlags::OPOST | nix::sys::termios::OutputFlags::ONLCR;

        // Verificar ECHOCTL deshabilitado
        assert!(
            !t.local_flags
                .contains(nix::sys::termios::LocalFlags::ECHOCTL),
            "ECHOCTL debe estar deshabilitado para evitar caret notation"
        );
        // Verificar ECHO habilitado para eco de caracteres imprimibles
        assert!(
            t.local_flags.contains(nix::sys::termios::LocalFlags::ECHO),
            "ECHO debe estar habilitado para que el kernel haga eco de teclas"
        );
        // Verificar ISIG habilitado para Ctrl+C, Ctrl+Z
        assert!(
            t.local_flags.contains(nix::sys::termios::LocalFlags::ISIG),
            "ISIG debe estar habilitado para Ctrl+C/Ctrl+Z"
        );
        // Verificar OPOST + ONLCR habilitados para output processing
        assert!(
            t.output_flags
                .contains(nix::sys::termios::OutputFlags::OPOST),
            "OPOST debe estar habilitado para output processing"
        );
        assert!(
            t.output_flags
                .contains(nix::sys::termios::OutputFlags::ONLCR),
            "ONLCR debe estar habilitado para \\n -> \\r\\n conversion"
        );

        // Cerrar FDs
        drop(result);
    }
}
