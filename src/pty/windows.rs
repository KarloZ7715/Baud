//! Backend de sesión ConPTY para Windows.

use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{
    CloseHandle, FreeLibrary, LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING,
    ERROR_PIPE_NOT_CONNECTED, FALSE, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, S_OK, TRUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
    OPEN_EXISTING, PIPE_ACCESS_INBOUND,
};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, CreatePipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
    ResetEvent, SetEvent, TerminateProcess, UpdateProcThreadAttribute, WaitForMultipleObjects,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::contract::{SessionBackend, WakeSource};
use super::{ProcessConfig, SessionKind};

const DEFAULT_ROWS: i16 = 24;
const DEFAULT_COLS: i16 = 80;

type CreatePseudoConsoleFn =
    unsafe extern "system" fn(COORD, HANDLE, HANDLE, u32, *mut HPCON) -> i32;
type ResizePseudoConsoleFn = unsafe extern "system" fn(HPCON, COORD) -> i32;
type ClosePseudoConsoleFn = unsafe extern "system" fn(HPCON);

/// De dónde salió la API ConPTY en uso.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConptySource {
    /// `conpty.dll` junto al ejecutable (la versión empaquetada por Baud).
    Bundled,
    /// kernel32/kernelbase del sistema operativo.
    Os,
}

/// Resolución de Create/Resize/Close: par empaquetado o API del OS.
///
/// `load` no falla: si no hay dll o no exporta los símbolos, se usa el OS.
pub struct ConptyApi {
    create: CreatePseudoConsoleFn,
    resize: ResizePseudoConsoleFn,
    close: ClosePseudoConsoleFn,
    source: ConptySource,
}

impl ConptyApi {
    /// Busca `conpty.dll` junto al ejecutable. Una sola vez al arrancar
    /// (`OnceLock`); el resto del módulo llama a través de esta API.
    pub fn load() -> &'static ConptyApi {
        static API: OnceLock<ConptyApi> = OnceLock::new();
        API.get_or_init(|| {
            if let Some(bundled) = Self::try_load_bundled() {
                tracing::info!("conpty: usando el par empaquetado (conpty.dll)");
                return bundled;
            }
            tracing::info!("conpty: usando el ConPTY del sistema operativo");
            Self::os_api()
        })
    }

    pub fn source(&self) -> ConptySource {
        self.source
    }

    fn os_api() -> Self {
        Self {
            create: CreatePseudoConsole,
            resize: ResizePseudoConsole,
            close: ClosePseudoConsole,
            source: ConptySource::Os,
        }
    }

    /// Carga `conpty.dll` con ruta absoluta junto al exe. El nombre suelto
    /// buscaría en PATH y podría resolver otra dll.
    fn try_load_bundled() -> Option<Self> {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(err) => {
                tracing::debug!("conpty: current_exe fallo: {err}");
                return None;
            }
        };
        let dir = exe.parent()?;
        let dll_path = dir.join("conpty.dll");
        if !dll_path.is_file() {
            tracing::debug!("conpty: no hay conpty.dll junto al ejecutable");
            return None;
        }
        // Sin OpenConsole.exe la dll empaquetada no puede alojar la sesión.
        if !dir.join("OpenConsole.exe").is_file() {
            tracing::debug!("conpty: conpty.dll presente sin OpenConsole.exe; se ignora");
            return None;
        }

        let wide = wide_null(dll_path.as_os_str());
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            tracing::debug!("conpty: LoadLibraryW fallo: {}", io::Error::last_os_error());
            return None;
        }

        let create = unsafe { GetProcAddress(module, b"CreatePseudoConsole\0".as_ptr()) };
        let resize = unsafe { GetProcAddress(module, b"ResizePseudoConsole\0".as_ptr()) };
        let close = unsafe { GetProcAddress(module, b"ClosePseudoConsole\0".as_ptr()) };
        match (create, resize, close) {
            (Some(create), Some(resize), Some(close)) => {
                // No FreeLibrary: los punteros viven tanto como el proceso.
                let _keep_loaded = module;
                // SAFETY: conpty.dll exporta estas tres con la misma firma
                // que kernel32.
                Some(Self {
                    create: unsafe { mem::transmute::<_, CreatePseudoConsoleFn>(create) },
                    resize: unsafe { mem::transmute::<_, ResizePseudoConsoleFn>(resize) },
                    close: unsafe { mem::transmute::<_, ClosePseudoConsoleFn>(close) },
                    source: ConptySource::Bundled,
                })
            }
            _ => {
                tracing::debug!(
                    "conpty: GetProcAddress no resolvio los simbolos; se ignora la dll"
                );
                unsafe {
                    let _ = FreeLibrary(module);
                }
                None
            }
        }
    }
}

/// Sesión respaldada por ConPTY.
pub struct Pty {
    hpcon: HPCON,
    /// El host escribe aquí (stdin del hijo vía ConPTY).
    conin: HANDLE,
    /// El host lee aquí (stdout del hijo vía ConPTY). Admite I/O superpuesta.
    conout: HANDLE,
    process: HANDLE,
    thread: HANDLE,
    /// Lectura superpuesta permanente sobre `conout`.
    pending: PendingRead,
}

/// Estado de la lectura superpuesta sobre conout: un ReadFile siempre en
/// vuelo cuyo evento se espera con WaitForMultipleObjects, sin polling.
struct PendingRead {
    overlapped: OVERLAPPED,
    /// Evento manual-reset de la lectura en vuelo.
    event: HANDLE,
    buf: Vec<u8>,
    in_flight: bool,
    /// Bytes completados pendientes de entregar (`delivered..ready`).
    ready: usize,
    delivered: usize,
    /// Pipe roto (el hijo terminó): la próxima lectura devuelve EOF.
    eof: bool,
}

impl PendingRead {
    fn new() -> io::Result<Self> {
        let event = unsafe { CreateEventW(ptr::null(), TRUE, FALSE, ptr::null()) };
        if event.is_null() || event == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut overlapped: OVERLAPPED = unsafe { mem::zeroed() };
        overlapped.hEvent = event;
        Ok(Self {
            overlapped,
            event,
            buf: vec![0u8; 64 * 1024],
            in_flight: false,
            ready: 0,
            delivered: 0,
            eof: false,
        })
    }

    fn has_ready(&self) -> bool {
        self.ready > self.delivered
    }

    /// Emite la lectura superpuesta si no hay una en vuelo ni bytes listos.
    fn issue(&mut self, conout: HANDLE) -> io::Result<()> {
        if self.in_flight || self.has_ready() || self.eof {
            return Ok(());
        }
        unsafe {
            let _ = ResetEvent(self.event);
        }
        let ok = unsafe {
            ReadFile(
                conout,
                self.buf.as_mut_ptr() as *mut _,
                self.buf.len() as u32,
                ptr::null_mut(),
                &mut self.overlapped,
            )
        };
        if ok == FALSE {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(code) if code == ERROR_IO_PENDING as i32 => {}
                Some(code)
                    if code == ERROR_BROKEN_PIPE as i32
                        || code == ERROR_PIPE_NOT_CONNECTED as i32 =>
                {
                    self.eof = true;
                    return Ok(());
                }
                _ => return Err(err),
            }
        }
        // La lectura completada en línea y la pendiente se resuelven por el
        // mismo camino: GetOverlappedResult en try_complete.
        self.in_flight = true;
        Ok(())
    }

    /// `Some(n)` si la lectura en vuelo completó (0 = pipe roto), `None` si
    /// sigue pendiente.
    fn try_complete(&mut self, conout: HANDLE) -> io::Result<Option<usize>> {
        debug_assert!(self.in_flight);
        let mut n = 0u32;
        let ok = unsafe { GetOverlappedResult(conout, &self.overlapped, &mut n, FALSE) };
        if ok == FALSE {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(code) if code == ERROR_IO_INCOMPLETE as i32 => return Ok(None),
                Some(code)
                    if code == ERROR_BROKEN_PIPE as i32
                        || code == ERROR_PIPE_NOT_CONNECTED as i32 =>
                {
                    self.in_flight = false;
                    self.eof = true;
                    return Ok(Some(0));
                }
                _ => return Err(err),
            }
        }
        self.in_flight = false;
        self.ready = n as usize;
        self.delivered = 0;
        Ok(Some(n as usize))
    }

    fn take_into(&mut self, out: &mut [u8]) -> usize {
        let n = (self.ready - self.delivered).min(out.len());
        out[..n].copy_from_slice(&self.buf[self.delivered..self.delivered + n]);
        self.delivered += n;
        if self.delivered == self.ready {
            self.ready = 0;
            self.delivered = 0;
        }
        n
    }
}

impl Drop for PendingRead {
    fn drop(&mut self) {
        if self.event != INVALID_HANDLE_VALUE && !self.event.is_null() {
            unsafe {
                CloseHandle(self.event);
            }
            self.event = INVALID_HANDLE_VALUE;
        }
    }
}

unsafe impl Send for Pty {}

impl Pty {
    pub fn set_winsize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let size = COORD {
            X: cols as i16,
            Y: rows as i16,
        };
        let hr = unsafe { (ConptyApi::load().resize)(self.hpcon, size) };
        if hr != S_OK {
            Err(io::Error::from_raw_os_error(hr))
        } else {
            Ok(())
        }
    }

    /// Bloquea hasta que conout tenga datos o `wake` esté señalizado.
    ///
    /// Sin polling: la lectura superpuesta permanente y el evento de wake se
    /// esperan juntos con WaitForMultipleObjects, así el hilo dormido no
    /// despierta 20 veces por segundo ni añade hasta 50 ms por tecla.
    pub fn wait_ready(&mut self, wake: &ConPtyWake) -> io::Result<WaitReady> {
        if self.pending.has_ready() || self.pending.eof {
            return Ok(WaitReady {
                output: true,
                wake: wake.is_signaled(),
            });
        }
        self.pending.issue(self.conout)?;
        let handles = [self.pending.event, wake.handle()];
        let r = unsafe {
            WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), FALSE, INFINITE)
        };
        if r == WAIT_OBJECT_0 {
            self.pending.try_complete(self.conout)?;
            Ok(WaitReady {
                output: true,
                wake: wake.is_signaled(),
            })
        } else if r == WAIT_OBJECT_0 + 1 {
            Ok(WaitReady {
                output: false,
                wake: true,
            })
        } else {
            Err(io::Error::last_os_error())
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
        loop {
            // Entregar primero los bytes de una lectura ya completada.
            if self.pending.has_ready() {
                return Ok(self.pending.take_into(buf));
            }
            if self.pending.eof {
                return Ok(0);
            }
            if self.pending.in_flight {
                match self.pending.try_complete(self.conout)? {
                    Some(_) => continue,
                    None => {
                        return Err(io::Error::new(ErrorKind::WouldBlock, "conout sin datos"));
                    }
                }
            }
            // Sin lectura en vuelo: emitir una; puede completar en línea y la
            // resuelve la siguiente iteración.
            self.pending.issue(self.conout)?;
        }
    }

    fn set_nonblocking(&mut self) -> io::Result<()> {
        // La lectura superpuesta ya es no bloqueante por diseño.
        Ok(())
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.force_kill();
        // Cancelar la lectura superpuesta antes de cerrar: sin CancelIoEx el
        // kernel podría completarla sobre el buffer/OVERLAPPED ya liberados.
        if self.conout != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CancelIoEx(self.conout, ptr::null());
                if self.pending.in_flight {
                    let mut n = 0u32;
                    let _ =
                        GetOverlappedResult(self.conout, &self.pending.overlapped, &mut n, TRUE);
                }
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
                (ConptyApi::load().close)(self.hpcon);
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
        ..ProcessConfig::default()
    })
}

pub fn spawn_with(cfg: &ProcessConfig) -> io::Result<Pty> {
    let mut cfg = cfg.clone();
    cfg.apply_shell_integration();
    let cfg = &cfg;
    let size = COORD {
        X: DEFAULT_COLS,
        Y: DEFAULT_ROWS,
    };

    let (conin_read, conin_write) = create_pipe_pair()?;
    let (conout_read, conout_write) = match create_overlapped_read_pipe() {
        Ok(pair) => pair,
        Err(e) => {
            unsafe {
                CloseHandle(conin_read);
                CloseHandle(conin_write);
            }
            return Err(e);
        }
    };

    let api = ConptyApi::load();
    let mut hpcon: HPCON = 0;
    let hr = unsafe { (api.create)(size, conin_read, conout_write, 0, &mut hpcon) };
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

    let pending = match PendingRead::new() {
        Ok(p) => p,
        Err(e) => {
            unsafe {
                (api.close)(hpcon);
                CloseHandle(conin_write);
                CloseHandle(conout_read);
            }
            return Err(e);
        }
    };

    let mut attr_size: usize = 0;
    unsafe {
        let _ = InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut attr_size);
    }
    let mut attr_buf = vec![0u8; attr_size];
    let attr_list = attr_buf.as_mut_ptr() as *mut _;

    let ok = unsafe { InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_size) };
    if ok == FALSE {
        unsafe {
            (api.close)(hpcon);
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
            (api.close)(hpcon);
            CloseHandle(conin_write);
            CloseHandle(conout_read);
        }
        return Err(io::Error::last_os_error());
    }

    let (shell, args, cwd) = match cfg.kind {
        SessionKind::Native => (
            cfg.shell.clone(),
            cfg.args.clone(),
            cfg.working_directory.clone(),
        ),
        SessionKind::Wsl => {
            let exe = super::wsl::wsl_exe_path()?;
            super::wsl::preflight(&exe)?;
            let argv = super::wsl::build_wsl_argv(
                cfg.distro.as_deref(),
                cfg.wsl_cwd.as_deref(),
                None,
                None,
            );
            (
                exe.to_string_lossy().into_owned(),
                argv,
                cfg.working_directory.clone(),
            )
        }
    };

    let mut cmdline = build_cmdline(&shell, &args);
    let cwd = cwd.as_ref().map(|d| wide_null(OsStr::new(d)));

    let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
    startup.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.lpAttributeList = attr_list;

    let mut proc_info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    let env_block = build_env_block(&cfg.env);
    let creation_flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
    let env_ptr = env_block.as_ptr() as *mut _;

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
            (api.close)(hpcon);
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
        pending,
    })
}

/// Crea el par de pipes de conout con nombre único. `CreatePipe` no admite
/// `FILE_FLAG_OVERLAPPED`, así que el extremo de lectura (el nuestro) es un
/// named pipe superpuesto; el de escritura (para ConPTY) sigue síncrono.
fn create_overlapped_read_pipe() -> io::Result<(HANDLE, HANDLE)> {
    // DACL que solo concede acceso al propietario (el usuario actual).
    let sddl = wide_null(OsStr::new("D:P(A;;GA;;;OW)"));
    let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            ptr::null_mut(),
        )
    };
    if ok == FALSE {
        return Err(io::Error::last_os_error());
    }
    let result = create_overlapped_read_pipe_with_sd(sd);
    unsafe {
        let _ = LocalFree(sd);
    }
    result
}

fn create_overlapped_read_pipe_with_sd(sd: PSECURITY_DESCRIPTOR) -> io::Result<(HANDLE, HANDLE)> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    for _ in 0..4 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let name = format!(r"\\.\pipe\baud-{}-{n}-{nanos}", std::process::id());
        let wide = wide_null(OsStr::new(&name));
        let sa = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: FALSE,
        };
        let read = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                64 * 1024,
                64 * 1024,
                0,
                &sa,
            )
        };
        if read == INVALID_HANDLE_VALUE {
            let err = io::Error::last_os_error();
            // Nombre ocupado (colisión o squatting): reintentar con otro.
            if err.raw_os_error() == Some(231) || err.raw_os_error() == Some(5) {
                continue;
            }
            return Err(err);
        }
        // El extremo que recibe ConPTY se abre síncrono y heredable.
        let sa_inherit = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: TRUE,
        };
        let write = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_WRITE,
                0,
                &sa_inherit,
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if write == INVALID_HANDLE_VALUE {
            let err = io::Error::last_os_error();
            unsafe {
                CloseHandle(read);
            }
            return Err(err);
        }
        return Ok((read, write));
    }
    Err(io::Error::new(
        ErrorKind::AddrInUse,
        "sin nombre de pipe disponible",
    ))
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

fn build_env_block(extra: &[(String, String)]) -> Vec<u16> {
    let mut block = Vec::new();
    let mut seen = std::collections::HashSet::<std::ffi::OsString>::new();
    // TERM/COLORTERM se fuerzan al final (misma política que Unix).
    let forced = ["TERM", "COLORTERM"];

    for (k, v) in extra {
        if forced.iter().any(|f| k.eq_ignore_ascii_case(f)) {
            continue;
        }
        let key_up = OsStr::new(k).to_ascii_uppercase();
        if seen.insert(key_up) {
            block.extend(OsStr::new(k).encode_wide());
            block.push(u16::from(b'='));
            block.extend(OsStr::new(v).encode_wide());
            block.push(0);
        }
    }
    for (k, v) in std::env::vars_os() {
        if forced.iter().any(|f| k.eq_ignore_ascii_case(OsStr::new(f))) {
            continue;
        }
        let key_up = k.to_ascii_uppercase();
        if seen.insert(key_up) {
            block.extend(k.encode_wide());
            block.push(u16::from(b'='));
            block.extend(v.encode_wide());
            block.push(0);
        }
    }
    for (k, v) in [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")] {
        block.extend(OsStr::new(k).encode_wide());
        block.push(u16::from(b'='));
        block.extend(OsStr::new(v).encode_wide());
        block.push(0);
    }
    block.push(0);
    block
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

    #[test]
    fn sin_dll_junto_al_exe_se_usa_el_conpty_del_os() {
        // En el runner de CI no hay conpty.dll junto al binario de test.
        let api = ConptyApi::load();
        assert_eq!(api.source(), ConptySource::Os);
    }

    #[test]
    #[ignore = "requiere conpty.dll y OpenConsole.exe junto al binario de test"]
    fn con_dll_junto_al_exe_se_usa_el_par_empaquetado() {
        let api = ConptyApi::load();
        assert_eq!(api.source(), ConptySource::Bundled);
    }
}
