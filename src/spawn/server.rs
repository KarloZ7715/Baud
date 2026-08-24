//! Listener JSON Lines del socket de spawn (`hello` + `new_tab`).

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::io::{self, BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::json;
use winit::event_loop::EventLoopProxy;

use super::{socket_path_in, token_path_in, SpawnListener, SpawnParams};
use crate::remote::{Request, Response};
use crate::window::UserEvent;

/// El primer `new_tab` puede esperar a crear ventana y GPU.
const NEW_TAB_TIMEOUT: Duration = Duration::from_secs(60);

pub type SpawnHandler = Arc<dyn Fn(SpawnParams, mpsc::Sender<Response>) + Send + Sync>;

/// Handle del hilo listener. Al dropearlo se apaga el accept y el
/// `SpawnListener` (en el hilo) borra sock y token.
pub struct SpawnServerHandle {
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    dir: std::path::PathBuf,
}

impl Drop for SpawnServerHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        wake_listener(&self.dir);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

pub fn serve(listener: SpawnListener, proxy: EventLoopProxy<UserEvent>) -> SpawnServerHandle {
    serve_with_handler(
        listener,
        Arc::new(move |params, tx| {
            let _ = proxy.send_event(UserEvent::SpawnTab { params, tx });
        }),
    )
}

pub fn serve_with_handler(listener: SpawnListener, handler: SpawnHandler) -> SpawnServerHandle {
    let dir = listener.dir().to_path_buf();
    let token = std::fs::read_to_string(token_path_in(&dir))
        .unwrap_or_default()
        .trim()
        .to_string();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_thread = Arc::clone(&shutdown);
    let thread = std::thread::spawn(move || {
        accept_loop(listener, token, shutdown_thread, handler);
    });
    SpawnServerHandle {
        shutdown,
        thread: Some(thread),
        dir,
    }
}

fn spawn_params_from_json(v: &serde_json::Value) -> SpawnParams {
    SpawnParams {
        command: v.get("command").and_then(|c| {
            c.as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
        }),
        working_directory: v
            .get("working_directory")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        title: v.get("title").and_then(|x| x.as_str()).map(str::to_string),
        hold: v.get("hold").and_then(|x| x.as_bool()).unwrap_or(false),
        app_id: v.get("app_id").and_then(|x| x.as_str()).map(str::to_string),
    }
}

fn handle_client<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    expected_token: &str,
    handler: &SpawnHandler,
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
                let _ = write_response(
                    &mut writer,
                    &Response::err(req.id, "auth", "hello with a valid token is required"),
                );
                break;
            }
            authed = true;
            let body = json!({
                "pid": std::process::id(),
            });
            let _ = write_response(&mut writer, &Response::ok(req.id, body));
            continue;
        }
        let resp = match req.method.as_str() {
            "new_tab" => {
                let params = spawn_params_from_json(&req.params);
                let (tx, rx) = mpsc::channel();
                handler(params, tx);
                match rx.recv_timeout(NEW_TAB_TIMEOUT) {
                    Ok(r) => r,
                    Err(_) => Response::err(req.id, "timeout", "event loop did not answer"),
                }
            }
            other => Response::err(req.id, "unknown_method", format!("unknown method: {other}")),
        };
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
fn accept_loop(
    listener: SpawnListener,
    token: String,
    shutdown: Arc<AtomicBool>,
    handler: SpawnHandler,
) {
    let unix = listener.listener();
    while !shutdown.load(Ordering::SeqCst) {
        match unix.accept() {
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
}

#[cfg(unix)]
fn wake_listener(dir: &std::path::Path) {
    use std::os::unix::net::UnixStream;
    let _ = UnixStream::connect(socket_path_in(dir));
}

#[cfg(windows)]
fn accept_loop(
    mut listener: SpawnListener,
    token: String,
    shutdown: Arc<AtomicBool>,
    handler: SpawnHandler,
) {
    let pipe_name = listener.pipe_name().to_string();
    let mut pending = listener.take_first_pipe();
    while !shutdown.load(Ordering::SeqCst) {
        let file = match pending.take() {
            Some(file) => match wait_pipe_client(&file) {
                Ok(()) => file,
                Err(_) => {
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    pending = Some(file);
                    continue;
                }
            },
            None => match accept_pipe(&pipe_name) {
                Ok(file) => file,
                Err(_) => {
                    if shutdown.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            },
        };
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
}

#[cfg(windows)]
fn wait_pipe_client(file: &std::fs::File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FALSE;
    use windows_sys::Win32::System::Pipes::ConnectNamedPipe;

    let handle = file.as_raw_handle();
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected == FALSE {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(535) {
            return Err(err);
        }
    }
    Ok(())
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
fn wake_listener(dir: &std::path::Path) {
    use std::fs::OpenOptions;
    let target = std::fs::read_to_string(socket_path_in(dir)).unwrap_or_default();
    let target = target.trim();
    if target.is_empty() {
        return;
    }
    let _ = OpenOptions::new().read(true).write(true).open(target);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn::{socket_path_in, token_path_in, try_bind_in, BindOutcome};

    #[test]
    fn serve_should_accept_hello_and_new_tab() {
        let tmp = tempfile::tempdir().unwrap();
        let BindOutcome::Held(listener) = try_bind_in(tmp.path()).unwrap() else {
            panic!("bind");
        };
        let (proxy_tx, proxy_rx) = std::sync::mpsc::channel();
        let _h = serve_with_handler(
            listener,
            Arc::new(move |params, tx| {
                let _ = proxy_tx.send(params);
                let _ = tx.send(crate::remote::Response::ok(2, serde_json::json!({})));
            }),
        );
        let token = std::fs::read_to_string(token_path_in(tmp.path())).unwrap();
        let mut client = crate::remote::server::Client::connect(
            socket_path_in(tmp.path()).to_str().unwrap(),
            token.trim(),
        )
        .unwrap();
        let resp = client
            .call(crate::remote::Request {
                id: 2,
                method: "new_tab".into(),
                params: serde_json::json!({"hold": true}),
            })
            .unwrap();
        assert!(resp.is_ok());
        let params = proxy_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(params.hold);
    }
}
