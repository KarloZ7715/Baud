//! Backend de sesión ConPTY para Windows.

use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{
    CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE, S_OK, TRUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
    ResetEvent, SetEvent, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};

use super::contract::{SessionBackend, WakeSource};
use super::ProcessConfig;

const DEFAULT_ROWS: i16 = 24;
const DEFAULT_COLS: i16 = 80;

/// Sesión respaldada por ConPTY.
pub struct Pty {
    hpcon: HPCON,
    /// El host escribe aquí (stdin del hijo vía ConPTY).
    conin: HANDLE,
    /// El host lee aquí (stdout del hijo vía ConPTY).
    conout: HANDLE,
    process: HANDLE,
    thread: HANDLE,
}

unsafe impl Send for Pty {}

impl Pty {
    pub fn set_winsize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let size = COORD {
            X: cols as i16,
            Y: rows as i16,
        };
        let hr = unsafe { ResizePseudoConsole(self.hpcon, size) };
        if hr != S_OK {
            Err(io::Error::from_raw_os_error(hr))
        } else {
            Ok(())
        }
    }

    /// Bloquea hasta que conout tenga datos o `wake` esté señalizado.
    pub fn wait_ready(&self, wake: &ConPtyWake) -> io::Result<WaitReady> {
        loop {
            let mut avail = 0u32;
            let peek_ok = unsafe {
                PeekNamedPipe(
                    self.conout,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut avail,
                    ptr::null_mut(),
                )
            };
            if peek_ok == FALSE {
                let err = io::Error::last_os_error();
                // ERROR_BROKEN_PIPE
                if err.raw_os_error() == Some(109) {
                    return Ok(WaitReady {
                        output: true,
                        wake: wake.is_signaled(),
                    });
                }
                return Err(err);
            }
            let woke = wake.is_signaled();
            if avail > 0 || woke {
                return Ok(WaitReady {
                    output: avail > 0,
                    wake: woke,
                });
            }
            let wait = unsafe { WaitForSingleObject(wake.handle(), 50) };
            if wait == WAIT_OBJECT_0 {
                return Ok(WaitReady {
                    output: false,
                    wake: true,
                });
            }
        }
    }
}

pub struct WaitReady {
    pub output: bool,
    pub wake: bool,
}

impl SessionBackend for Pty {
    fn spawn(cfg: &ProcessConfig) -> io::Result<Self> {
        spawn_with(cfg)
    }

    fn write_input(&mut self, data: &[u8]) -> io::Result<()> {
        write_all_handle(self.conin, data)
    }

    fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        self.set_winsize(rows, cols)
    }

    fn interrupt(&mut self) -> io::Result<()> {
        self.write_input(&[0x03])
    }

    fn shutdown_graceful(&mut self) -> bool {
        if self.conin != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.conin);
            }
            self.conin = INVALID_HANDLE_VALUE;
            true
        } else {
            false
        }
    }

    fn force_kill(&mut self) {
        if self.process != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = TerminateProcess(self.process, 1);
            }
        }
    }

    fn read_output(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                self.conout,
                buf.as_mut_ptr() as *mut _,
                buf.len() as u32,
                &mut read,
                ptr::null_mut(),
            )
        };
        if ok == FALSE {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(232) || err.kind() == ErrorKind::WouldBlock {
                return Err(io::Error::new(ErrorKind::WouldBlock, err));
            }
            if err.raw_os_error() == Some(109) {
                return Ok(0);
            }
            return Err(err);
        }
        Ok(read as usize)
    }

    fn set_nonblocking(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.force_kill();
        // Cerrar conout antes de ClosePseudoConsole para no deadlockear
        // esperando un pipe de lectura aún vivo en este mismo hilo.
        if self.conout != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.conout);
            }
            self.conout = INVALID_HANDLE_VALUE;
        }
        if self.conin != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.conin);
            }
            self.conin = INVALID_HANDLE_VALUE;
        }
        if self.hpcon != 0 {
            unsafe {
                ClosePseudoConsole(self.hpcon);
            }
            self.hpcon = 0;
        }
        if self.thread != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.thread);
            }
            self.thread = INVALID_HANDLE_VALUE;
        }
        if self.process != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.process);
            }
            self.process = INVALID_HANDLE_VALUE;
        }
    }
}

/// Evento de reset manual para despertar el hilo PTY en Windows.
pub struct ConPtyWake {
    handle: HANDLE,
    signaled: AtomicBool,
}

unsafe impl Send for ConPtyWake {}
unsafe impl Sync for ConPtyWake {}

impl ConPtyWake {
    pub fn new() -> io::Result<Self> {
        let handle = unsafe { CreateEventW(ptr::null(), TRUE, FALSE, ptr::null()) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle,
            signaled: AtomicBool::new(false),
        })
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
    }

    pub fn is_signaled(&self) -> bool {
        self.signaled.load(Ordering::Acquire)
    }
}

impl Drop for ConPtyWake {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

impl WakeSource for ConPtyWake {
    fn wake(&self) {
        self.signaled.store(true, Ordering::Release);
        unsafe {
            let _ = SetEvent(self.handle);
        }
    }

    fn drain(&self) {
        self.signaled.store(false, Ordering::Release);
        unsafe {
            let _ = ResetEvent(self.handle);
        }
    }
}

/// Lanza usando tamaño por defecto 24x80.
pub fn spawn(shell: &str, args: &[&str]) -> io::Result<Pty> {
    spawn_with(&ProcessConfig {
        shell: shell.into(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        working_directory: None,
        env: Vec::new(),
        startup_command: None,
        login_shell: false,
    })
}

pub fn spawn_with(cfg: &ProcessConfig) -> io::Result<Pty> {
    let size = COORD {
        X: DEFAULT_COLS,
        Y: DEFAULT_ROWS,
    };

    let (conin_read, conin_write) = create_pipe_pair()?;
    let (conout_read, conout_write) = create_pipe_pair()?;

    let mut hpcon: HPCON = 0;
    let hr = unsafe { CreatePseudoConsole(size, conin_read, conout_write, 0, &mut hpcon) };
    if hr != S_OK {
        unsafe {
            CloseHandle(conin_read);
            CloseHandle(conin_write);
            CloseHandle(conout_read);
            CloseHandle(conout_write);
        }
        return Err(io::Error::from_raw_os_error(hr));
    }

    unsafe {
        CloseHandle(conin_read);
        CloseHandle(conout_write);
    }

    let mut attr_size: usize = 0;
    unsafe {
        let _ = InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut attr_size);
    }
    let mut attr_buf = vec![0u8; attr_size];
    let attr_list = attr_buf.as_mut_ptr() as *mut _;

    let ok = unsafe { InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) };
    if ok == FALSE {
        unsafe {
            ClosePseudoConsole(hpcon);
            CloseHandle(conin_write);
            CloseHandle(conout_read);
        }
        return Err(io::Error::last_os_error());
    }

    let ok = unsafe {
        UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            hpcon as *mut _,
            mem::size_of::<HPCON>(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ok == FALSE {
        unsafe {
            DeleteProcThreadAttributeList(attr_list);
            ClosePseudoConsole(hpcon);
            CloseHandle(conin_write);
            CloseHandle(conout_read);
        }
        return Err(io::Error::last_os_error());
    }

    let mut cmdline = build_cmdline(&cfg.shell, &cfg.args);
    let cwd = cfg
        .working_directory
        .as_ref()
        .map(|d| wide_null(OsStr::new(d)));

    let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
    startup.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.lpAttributeList = attr_list;

    let mut proc_info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    let mut creation_flags = EXTENDED_STARTUPINFO_PRESENT;

    let env_block = build_env_block(&cfg.env);
    let env_ptr = if let Some(ref block) = env_block {
        creation_flags |= CREATE_UNICODE_ENVIRONMENT;
        block.as_ptr() as *mut _
    } else {
        ptr::null_mut()
    };

    let ok = unsafe {
        CreateProcessW(
            ptr::null(),
            cmdline.as_mut_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
            FALSE,
            creation_flags,
            env_ptr,
            cwd.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
            &mut startup.StartupInfo as *mut STARTUPINFOW,
            &mut proc_info,
        )
    };

    unsafe {
        DeleteProcThreadAttributeList(attr_list);
    }

    if ok == FALSE {
        unsafe {
            ClosePseudoConsole(hpcon);
            CloseHandle(conin_write);
            CloseHandle(conout_read);
        }
        return Err(io::Error::last_os_error());
    }

    Ok(Pty {
        hpcon,
        conin: conin_write,
        conout: conout_read,
        process: proc_info.hProcess,
        thread: proc_info.hThread,
    })
}

fn create_pipe_pair() -> io::Result<(HANDLE, HANDLE)> {
    let mut read = INVALID_HANDLE_VALUE;
    let mut write = INVALID_HANDLE_VALUE;
    let ok = unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) };
    if ok == FALSE {
        Err(io::Error::last_os_error())
    } else {
        Ok((read, write))
    }
}

fn write_all_handle(handle: HANDLE, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle,
                data.as_ptr() as *const _,
                data.len() as u32,
                &mut written,
                ptr::null_mut(),
            )
        };
        if ok == FALSE {
            return Err(io::Error::last_os_error());
        }
        if written == 0 {
            return Err(io::Error::new(
                ErrorKind::WriteZero,
                "WriteFile wrote zero bytes",
            ));
        }
        data = &data[written as usize..];
    }
    Ok(())
}

fn wide_null(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn build_cmdline(shell: &str, args: &[String]) -> Vec<u16> {
    let mut cmd = String::new();
    quote_arg(&mut cmd, shell);
    for a in args {
        cmd.push(' ');
        quote_arg(&mut cmd, a);
    }
    wide_null(OsStr::new(&cmd))
}

fn quote_arg(out: &mut String, arg: &str) {
    if arg.is_empty() || arg.chars().any(|c| c.is_whitespace() || c == '"') {
        out.push('"');
        for c in arg.chars() {
            if c == '"' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
    } else {
        out.push_str(arg);
    }
}

fn build_env_block(extra: &[(String, String)]) -> Option<Vec<u16>> {
    if extra.is_empty() {
        return None;
    }
    let mut block = Vec::new();
    let mut seen = std::collections::HashSet::<std::ffi::OsString>::new();
    for (k, v) in extra {
        let key_up = OsStr::new(k).to_ascii_uppercase();
        if seen.insert(key_up) {
            block.extend(OsStr::new(k).encode_wide());
            block.push(u16::from(b'='));
            block.extend(OsStr::new(v).encode_wide());
            block.push(0);
        }
    }
    for (k, v) in std::env::vars_os() {
        let key_up = k.to_ascii_uppercase();
        if seen.insert(key_up) {
            block.extend(k.encode_wide());
            block.push(u16::from(b'='));
            block.extend(v.encode_wide());
            block.push(0);
        }
    }
    for (k, v) in [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")] {
        let key_up = OsStr::new(k).to_ascii_uppercase();
        if seen.insert(key_up) {
            block.extend(OsStr::new(k).encode_wide());
            block.push(u16::from(b'='));
            block.extend(OsStr::new(v).encode_wide());
            block.push(0);
        }
    }
    block.push(0);
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_arg_spaces() {
        let mut s = String::new();
        quote_arg(&mut s, r"C:\Program Files\pwsh.exe");
        assert!(s.starts_with('"'));
        assert!(s.ends_with('"'));
    }
}
