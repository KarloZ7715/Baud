#![cfg(windows)]
//! E2E: cerrar Baud con una TUI activa debe terminar con código 0.
//!
//! Requiere los binarios compilados; corre solo en CI con `--ignored`.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

#[test]
#[ignore = "necesita sesion grafica de Windows; corre en CI con --ignored"]
fn close_while_tui_running_exits_cleanly() {
    let exe = env!("CARGO_BIN_EXE_baud");
    let dummy = env!("CARGO_BIN_EXE_tui_dummy");
    // Sin --new-instance, `baud -e` es un cliente de spawn: pide tab y sale 0
    // antes de crear HWND. Este test cierra por PID del proceso GUI.
    let mut child = Command::new(exe)
        .args(["--new-instance", "-e", dummy])
        .env("RUST_BACKTRACE", "full")
        .spawn()
        .expect("lanzar baud");

    // wgpu en el runner de CI puede tardar varios segundos en `resumed`.
    // El HWND correcto es el de titulo "baud"; el primer HWND del PID suele
    // ser un helper de DXGI/IME que ignora WM_CLOSE.
    let hwnd = wait_for_baud_window(&mut child, Duration::from_secs(15));
    unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            assert_eq!(status.code(), Some(0), "baud debe cerrar con codigo 0");
            return;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("baud no termino en 15s tras WM_CLOSE (deadlock en teardown)");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_baud_window(child: &mut Child, timeout: Duration) -> HWND {
    let pid = child.id();
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(hwnd) = find_baud_window(pid) {
            return hwnd;
        }
        if let Some(status) = child.try_wait().expect("try_wait") {
            panic!(
                "baud salio antes de crear la ventana (codigo {:?})",
                status.code()
            );
        }
        if Instant::now() > deadline {
            panic!("no se encontro la ventana de baud (pid {pid}) en {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn find_baud_window(pid: u32) -> Option<HWND> {
    struct Ctx {
        pid: u32,
        hwnd: HWND,
    }

    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> i32 {
        let ctx = unsafe { &mut *(lp as *mut Ctx) };
        let mut wpid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut wpid) };
        if wpid != ctx.pid {
            return 1;
        }
        let title = window_title(hwnd);
        if title.eq_ignore_ascii_case("baud") {
            ctx.hwnd = hwnd;
            return 0;
        }
        1
    }

    let mut ctx = Ctx {
        pid,
        hwnd: std::ptr::null_mut(),
    };
    unsafe { EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM) };
    if ctx.hwnd.is_null() {
        None
    } else {
        Some(ctx.hwnd)
    }
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}
