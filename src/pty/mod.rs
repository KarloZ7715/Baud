use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::Stdio;

/// Wrapper minimalista sobre un file descriptor de PTY.
/// Implementa Read y Write delegando a nix::unistd.
pub struct Pty {
    fd: OwnedFd,
}

impl Pty {
    pub fn fd(&self) -> &OwnedFd {
        &self.fd
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
    Ok((Pty { fd: result.master }, Pty { fd: result.slave }))
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

    cmd.spawn()
        .map_err(|e| nix::errno::Errno::from_raw(e.raw_os_error().unwrap_or(0)))?;

    Ok(Pty { fd: result.master })
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
}
