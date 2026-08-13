#![cfg(windows)]
//! E2E: cerrar Baud con una TUI activa debe terminar con código 0.
//!
//! Requiere los binarios compilados; corre solo en CI con `--ignored`.

use std::process::Command;
use std::time::{Duration, Instant};

#[test]
#[ignore = "necesita sesion grafica de Windows; corre en CI con --ignored"]
fn close_while_tui_running_exits_cleanly() {
    let exe = env!("CARGO_BIN_EXE_baud");
    let dummy = env!("CARGO_BIN_EXE_tui_dummy");
    let mut child = Command::new(exe)
        .args(["-e", dummy])
        .env("RUST_BACKTRACE", "full")
        .spawn()
        .expect("lanzar baud");

    // Dar tiempo a que la ventana exista y la TUI pinte.
    std::thread::sleep(Duration::from_secs(4));

    // Cerrar vía WM_CLOSE a la ventana principal del proceso.
    close_main_window(child.id());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            assert_eq!(status.code(), Some(0), "baud debe cerrar con codigo 0");
            return;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("baud no termino en 10s tras WM_CLOSE (deadlock en teardown)");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn close_main_window(pid: u32) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    struct Ctx {
        pid: u32,
        sent: bool,
    }

    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> i32 {
        let ctx = unsafe { &mut *(lp as *mut Ctx) };
        let mut wpid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut wpid) };
        if wpid == ctx.pid {
            unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            ctx.sent = true;
            return 0;
        }
        1
    }

    let mut ctx = Ctx { pid, sent: false };
    unsafe { EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM) };
    assert!(ctx.sent, "no se encontro la ventana de baud (pid {pid})");
}
