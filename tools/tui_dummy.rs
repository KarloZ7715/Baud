//! TUI mínima para tests de cierre: alt-screen + redraws continuos.
//!
//! Sale sola cuando stdin llega a EOF (el PTY murió). Se usa como comando de
//! sesión en el test e2e de Windows para garantizar output continuo durante
//! el cierre de la ventana.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[?1049h\x1b[2J");
    let stdin_eof = Arc::new(AtomicBool::new(false));
    let eof = Arc::clone(&stdin_eof);
    std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            match std::io::stdin().read(&mut buf) {
                Ok(0) | Err(_) => {
                    eof.store(true, Ordering::Relaxed);
                    return;
                }
                Ok(_) => {}
            }
        }
    });
    let mut i = 0u64;
    while !stdin_eof.load(Ordering::Relaxed) {
        let _ = write!(out, "\x1b[H\x1b[Kframe {i}");
        let _ = out.flush();
        i += 1;
        std::thread::sleep(Duration::from_millis(16));
    }
    let _ = out.write_all(b"\x1b[?1049l");
}
