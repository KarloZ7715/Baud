//! Servidor IPC: JSON Lines sobre UDS (Unix) o named pipe (Windows).
//!
//! Opt-in: si `remote_control` es false el hilo ni se crea. El token vive en
//! un archivo 0600; poseerlo es la autorizacion. Solo local, nunca red.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::json;
use winit::event_loop::EventLoopProxy;

use crate::window::UserEvent;

use super::{Request, Response};

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

/// Rutas y token de una instancia viva.
pub struct Endpoint {
    pub token: String,
    /// Ruta del socket Unix, o nombre del named pipe en Windows.
    pub target: String,
    token_path: PathBuf,
    #[cfg(unix)]
    socket_path: PathBuf,
    #[cfg(windows)]
    pipe_path: PathBuf,
}

impl Endpoint {
    fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.token_path);
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.socket_path);
        }
        #[cfg(windows)]
        {
            let _ = std::fs::remove_file(&self.pipe_path);
        }
    }
}

/// Handle del hilo listener; al dropearlo se borra el socket y el token.
pub struct ServerHandle {
    shutdown: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
    endpoint: Endpoint,
}

impl ServerHandle {
    pub fn target(&self) -> &str {
        &self.endpoint.target
    }

    pub fn token(&self) -> &str {
        &self.endpoint.token
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        wake_listener(&self.endpoint);
        if let Some(h) = self.listener.take() {
            let _ = h.join();
        }
        self.endpoint.cleanup();
    }
}

/// Arranca el listener y registra token/socket. El handler corre en el hilo
/// de cada conexion y debe ser no bloqueante mas de unos segundos.
pub fn spawn(handler: Handler) -> io::Result<ServerHandle> {
    spawn_in(&runtime_dir()?, handler)
}

pub fn spawn_with_proxy(proxy: EventLoopProxy<UserEvent>) -> io::Result<ServerHandle> {
    spawn(Arc::new(move |req| proxy_handler(&proxy, req)))
}

pub fn spawn_in(dir: &Path, handler: Handler) -> io::Result<ServerHandle> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let token = generate_token()?;
    let pid = std::process::id();
    let endpoint = write_endpoint(dir, pid, &token)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let listener = Some(start_listener(
        endpoint.target.clone(),
        token.clone(),
        Arc::clone(&shutdown),
        handler,
    )?);
    Ok(ServerHandle {
        shutdown,
        listener,
        endpoint,
    })
}

fn proxy_handler(proxy: &EventLoopProxy<UserEvent>, req: Request) -> Response {
    let id = req.id;
    let wait_ms = if req.method == "wait_for" {
        req.params
            .get("timeout_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(5_000)
            .saturating_add(1_000)
    } else {
        5_000
    };
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy.send_event(UserEvent::Remote(req, tx)).is_err() {
        return Response::err(id, "disconnected", "event loop is gone");
    }
    match rx.recv_timeout(Duration::from_millis(wait_ms)) {
        Ok(r) => r,
        Err(_) => Response::err(id, "timeout", "event loop did not answer"),
    }
}

pub fn runtime_dir() -> io::Result<PathBuf> {
    #[cfg(unix)]
    {
        if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir).join("baud"));
            }
        }
        let uid = nix::unistd::Uid::current();
        let run = PathBuf::from(format!("/run/user/{uid}/baud"));
        if run.parent().is_some_and(|p| p.is_dir()) {
            return Ok(run);
        }
        Ok(PathBuf::from(format!("/tmp/baud-{uid}")))
    }
    #[cfg(windows)]
    {
        Ok(dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("baud")
            .join("runtime"))
    }
}

/// Busca la instancia mas reciente con token legible.
pub fn discover() -> io::Result<Option<(String, String)>> {
    discover_in(&runtime_dir()?)
}

pub fn discover_in(dir: &Path) -> io::Result<Option<(String, String)>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(None);
    };
    let mut socks: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            #[cfg(unix)]
            {
                s.ends_with(".sock")
            }
            #[cfg(windows)]
            {
                s.ends_with(".pipe")
            }
        })
        .collect();
    socks.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    let Some(latest) = socks.last() else {
        return Ok(None);
    };
    let stem = latest
        .path()
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let token_path = latest.path().with_file_name(format!("{stem}.token"));
    let token = std::fs::read_to_string(&token_path)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        Ok(Some((latest.path().to_string_lossy().into_owned(), token)))
    }
    #[cfg(windows)]
    {
        let pipe = std::fs::read_to_string(latest.path())?;
        Ok(Some((pipe.trim().to_string(), token)))
    }
}

fn write_endpoint(dir: &Path, pid: u32, token: &str) -> io::Result<Endpoint> {
    let token_path = dir.join(format!("{pid}.token"));
    std::fs::write(&token_path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))?;
        let socket_path = dir.join(format!("{pid}.sock"));
        let _ = std::fs::remove_file(&socket_path);
        Ok(Endpoint {
            token: token.to_string(),
            target: socket_path.to_string_lossy().into_owned(),
            token_path,
            socket_path,
        })
    }
    #[cfg(windows)]
    {
        let short = if token.len() >= 8 { &token[..8] } else { token };
        let target = format!(r"\\.\pipe\baud-{pid}-{short}");
        let pipe_path = dir.join(format!("{pid}.pipe"));
        std::fs::write(&pipe_path, &target)?;
        Ok(Endpoint {
            token: token.to_string(),
            target,
            token_path,
            pipe_path,
        })
    }
}

fn generate_token() -> io::Result<String> {
    let mut buf = [0u8; 32];
    fill_random(&mut buf)?;
    Ok(hex_encode(&buf))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn fill_random(buf: &mut [u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(buf)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!("BCryptGenRandom status {status}")))
        }
    }
}

fn handle_client<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    expected_token: &str,
    handler: &Handler,
) {
    let mut authed = false;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(
                    writer,
                    "{}",
                    json!({"id": 0, "err": {"code": "malformed", "msg": e.to_string()}})
                );
                let _ = writer.flush();
                continue;
            }
        };
        if !authed {
            let offered = req
                .params
                .get("token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if req.method != "hello" || offered != expected_token {
                let _ = writeln!(
                    writer,
                    "{}",
                    serde_json::to_string(&Response::err(
                        req.id,
                        "auth",
                        "hello with a valid token is required",
                    ))
                    .unwrap_or_else(|_| r#"{"id":0,"err":{"code":"auth","msg":"denied"}}"#.into())
                );
                let _ = writer.flush();
                break;
            }
            authed = true;
            let body = json!({
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
            });
            let _ = write_response(&mut writer, &Response::ok(req.id, body));
            continue;
        }
        let resp = handler(req);
        if write_response(&mut writer, &resp).is_err() {
            break;
        }
    }
}

fn write_response<W: Write>(writer: &mut W, resp: &Response) -> io::Result<()> {
    let line =
        serde_json::to_string(resp).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(writer, "{line}")?;
    writer.flush()
}

#[cfg(unix)]
fn start_listener(
    target: String,
    token: String,
    shutdown: Arc<AtomicBool>,
    handler: Handler,
) -> io::Result<JoinHandle<()>> {
    use std::os::unix::net::UnixListener;
    let listener = UnixListener::bind(&target)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700));
    }
    if let Err(e) = listener.set_nonblocking(true) {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    Ok(std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    let token = token.clone();
                    let handler = Arc::clone(&handler);
                    std::thread::spawn(move || {
                        let reader = BufReader::new(&stream);
                        handle_client(reader, &stream, &token, &handler);
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    }))
}

#[cfg(unix)]
fn wake_listener(endpoint: &Endpoint) {
    use std::os::unix::net::UnixStream;
    let _ = UnixStream::connect(&endpoint.socket_path);
}

#[cfg(windows)]
fn start_listener(
    target: String,
    token: String,
    shutdown: Arc<AtomicBool>,
    handler: Handler,
) -> io::Result<JoinHandle<()>> {
    Ok(std::thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            match accept_pipe(&target) {
                Ok(file) => {
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    let token = token.clone();
                    let handler = Arc::clone(&handler);
                    std::thread::spawn(move || {
                        let mut reader_file = match file.try_clone() {
                            Ok(f) => f,
                            Err(_) => return,
                        };
                        let reader = BufReader::new(&mut reader_file);
                        handle_client(reader, file, &token, &handler);
                    });
                }
                Err(_) => {
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }))
}

#[cfg(windows)]
fn accept_pipe(name: &str) -> io::Result<std::fs::File> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
    use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected == FALSE {
        let err = io::Error::last_os_error();
        // ERROR_PIPE_CONNECTED (535): el cliente ya estaba esperando.
        if err.raw_os_error() != Some(535) {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            return Err(err);
        }
    }
    let owned = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
    Ok(std::fs::File::from(owned))
}

#[cfg(windows)]
fn wake_listener(endpoint: &Endpoint) {
    use std::fs::OpenOptions;
    let _ = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&endpoint.target);
}

/// Cliente JSON Lines (tests, `baud mcp`, e2e).
pub struct Client {
    reader: BufReader<Box<dyn io::Read + Send>>,
    writer: Box<dyn io::Write + Send>,
}

impl Client {
    pub fn connect(target: &str, token: &str) -> io::Result<Self> {
        let mut last_err = None;
        for _ in 0..50 {
            match connect_raw(target) {
                Ok(mut client) => {
                    let hello = Request {
                        id: 0,
                        method: "hello".into(),
                        params: json!({ "token": token }),
                    };
                    let resp = client.call(hello)?;
                    if !resp.is_ok() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "remote control auth failed",
                        ));
                    }
                    return Ok(client);
                }
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        Err(last_err.unwrap_or_else(|| io::Error::other("connect failed")))
    }

    pub fn call(&mut self, req: Request) -> io::Result<Response> {
        let line = serde_json::to_string(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        let mut reply = String::new();
        if self.reader.read_line(&mut reply)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server closed the connection",
            ));
        }
        Response::from_json_line(reply.trim())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        writeln!(self.writer, "{line}")?;
        self.writer.flush()
    }

    pub fn read_line(&mut self) -> io::Result<String> {
        let mut reply = String::new();
        if self.reader.read_line(&mut reply)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server closed the connection",
            ));
        }
        Ok(reply)
    }
}

fn connect_raw(target: &str) -> io::Result<Client> {
    #[cfg(unix)]
    {
        let stream = std::os::unix::net::UnixStream::connect(target)?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let reader_stream = stream.try_clone()?;
        Ok(Client {
            reader: BufReader::new(Box::new(reader_stream)),
            writer: Box::new(stream),
        })
    }
    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        let file = OpenOptions::new().read(true).write(true).open(target)?;
        let reader_file = file.try_clone()?;
        Ok(Client {
            reader: BufReader::new(Box::new(reader_file)),
            writer: Box::new(file),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stub_handler() -> Handler {
        Arc::new(|req: Request| {
            if req.method == "screen_text" {
                Response::ok(req.id, json!({ "lines": ["stub"] }))
            } else {
                Response::err(req.id, "unknown_method", req.method)
            }
        })
    }

    #[test]
    fn hello_con_token_bueno_y_peticion_malformada() {
        let dir = tempfile::tempdir().unwrap();
        let server = spawn_in(dir.path(), stub_handler()).unwrap();
        let mut client = Client::connect(server.target(), server.token()).unwrap();

        client.write_line("{no json").unwrap();
        let reply = client.read_line().unwrap();
        let resp = Response::from_json_line(reply.trim()).unwrap();
        assert!(matches!(resp, Response::Err { ref code, .. } if code == "malformed"));

        let r = client
            .call(Request {
                id: 2,
                method: "screen_text".into(),
                params: json!({}),
            })
            .unwrap();
        assert_eq!(r.text_lines().unwrap(), vec!["stub".to_string()]);
    }

    #[test]
    fn hello_con_token_malo_cierra() {
        let dir = tempfile::tempdir().unwrap();
        let server = spawn_in(dir.path(), stub_handler()).unwrap();
        let err = match Client::connect(server.target(), "deadbeef") {
            Ok(_) => panic!("debia rechazar el token"),
            Err(e) => e,
        };
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
