//! Adjunta la consola del proceso padre en Windows para que `--version` y
//! `--help` sigan imprimiendo en cmd/PowerShell pese al subsistema GUI del
//! binario (`windows_subsystem = "windows"` en `src/main.rs`).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};

fn wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Intenta adjuntar la consola del proceso padre (cmd.exe, PowerShell,
/// Windows Terminal) y reabre stdin/stdout/stderr sobre ella.
///
/// Si el padre no tiene consola propia —el caso normal al lanzar desde el
/// Explorador o un acceso directo— `AttachConsole` falla y la función
/// devuelve `false` sin ruido: no se llama a `AllocConsole`, así que no
/// aparece ninguna ventana de consola adicional.
pub fn attach_parent_console() -> bool {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return false;
        }

        reopen_std_handle(&wide_null("CONOUT$"), STD_OUTPUT_HANDLE);
        reopen_std_handle(&wide_null("CONOUT$"), STD_ERROR_HANDLE);
        reopen_std_handle(&wide_null("CONIN$"), STD_INPUT_HANDLE);
    }
    true
}

/// # Safety
/// `name` debe ser una cadena UTF-16 terminada en nulo válida para `CreateFileW`.
unsafe fn reopen_std_handle(name: &[u16], std_handle: u32) {
    let handle: HANDLE = CreateFileW(
        name.as_ptr(),
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        ptr::null(),
        OPEN_EXISTING,
        0,
        ptr::null_mut(),
    );
    if handle != INVALID_HANDLE_VALUE {
        SetStdHandle(std_handle, handle);
    }
}
