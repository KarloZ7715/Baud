use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::Stdio;

/// Wrapper minimalista sobre un file descriptor de PTY.
/// Implementa Read y Write delegando a nix::unistd.
pub struct Pty {
    fd: OwnedFd,
    // ponytail: child_pid se guarda para SIGHUP en Drop (Ronda 4) y set_winsize.
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
    let result = nix::pty::openpty(None, None)?;

    // ponytail: duplicamos el slave 3 veces (stdin, stdout, stderr)
    let slave_stdin = nix::unistd::dup(&result.slave)?;
    let slave_stdout = nix::unistd::dup(&result.slave)?;

    let mut cmd = std::process::Command::new(shell);
    cmd.args(args);
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
}
