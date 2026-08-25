//! Query de detección del protocolo de gráficos bajo Xvfb.
//!
//! Arranca Baud y un hijo que envía `a=q`; el proceso debe ver `OK` en stdin.
//! Se ejecuta con `BAUD_X11_E2E=1` (job `x11 e2e`).

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const APP_ID: &str = "baud-graphics-query-e2e";

const QUERY_PY: &str = r#"
import os, sys, time
sys.stdout.buffer.write(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\")
sys.stdout.buffer.flush()
deadline = time.time() + 8
buf = b""
while time.time() < deadline:
    try:
        chunk = os.read(0, 1024)
    except BlockingIOError:
        chunk = b""
    if chunk:
        buf += chunk
        if b"OK" in buf and b"Gi=31" in buf:
            sys.exit(0)
    time.sleep(0.05)
sys.stderr.write(repr(buf) + "\n")
sys.exit(1)
"#;

#[test]
fn query_de_graficos_recibe_ok() {
    if std::env::var_os("BAUD_X11_E2E").is_none() {
        eprintln!("saltado: define BAUD_X11_E2E=1 con un servidor X disponible");
        return;
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args([
            "--new-instance",
            "--app-id",
            APP_ID,
            "-e",
            "python3",
            "-c",
            QUERY_PY,
        ])
        .env("BAUD_SKIP_CONSENT_UI", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("no se pudo lanzar baud");

    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "baud/python salió con {status:?}");
                return;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                panic!("timeout esperando la query de gráficos");
            }
            Err(e) => panic!("try_wait: {e}"),
        }
    }
}
