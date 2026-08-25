//! Un ClientMessage que no es WM_PROTOCOLS no debe cerrar la ventana.
//!
//! Regresión de v0.1.0: winit 0.30.13 traducía a `CloseRequested` cualquier
//! ClientMessage cuyo `data[0]` coincidiera con el átomo WM_DELETE_WINDOW, sin
//! mirar el `message_type`. En sesiones X11 completas eso cerraba Baud sola a
//! los pocos segundos de abrirla.
//!
//! Requiere un servidor X y una GPU (aunque sea por software). Se ejecuta solo
//! con `BAUD_X11_E2E=1`; el job `x11 e2e` de CI lo activa bajo Xvfb.

#![cfg(unix)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::wrapper::ConnectionExt as _;

const APP_ID: &str = "baud-x11-close-test";

/// Busca en el árbol de ventanas la primera cuyo WM_CLASS contenga `APP_ID`.
fn find_window<C: Connection>(conn: &C, root: Window) -> Option<Window> {
    let tree = conn.query_tree(root).ok()?.reply().ok()?;
    for child in tree.children {
        if let Ok(cookie) =
            conn.get_property(false, child, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        {
            if let Ok(prop) = cookie.reply() {
                if String::from_utf8_lossy(&prop.value).contains(APP_ID) {
                    return Some(child);
                }
            }
        }
        if let Some(found) = find_window(conn, child) {
            return Some(found);
        }
    }
    None
}

/// Espera hasta `timeout` a que la ventana de Baud exista.
fn wait_for_window<C: Connection>(conn: &C, root: Window, timeout: Duration) -> Option<Window> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(win) = find_window(conn, root) {
            return Some(win);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn spawn_baud() -> Child {
    Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(["--new-instance", "--app-id", APP_ID, "-e", "sleep", "300"])
        .env("BAUD_SKIP_CONSENT_UI", "1")
        .spawn()
        .expect("no se pudo lanzar el binario de baud")
}

#[test]
fn client_message_ajeno_no_cierra_la_ventana() {
    if std::env::var_os("BAUD_X11_E2E").is_none() {
        eprintln!("saltado: define BAUD_X11_E2E=1 con un servidor X disponible");
        return;
    }

    let (conn, screen_num) = x11rb::connect(None).expect("no hay servidor X disponible");
    let root = conn.setup().roots[screen_num].root;

    let mut child = spawn_baud();
    let win = match wait_for_window(&conn, root, Duration::from_secs(20)) {
        Some(win) => win,
        None => {
            let _ = child.kill();
            panic!("la ventana de baud no apareció en 20s");
        }
    };

    let delete = conn
        .intern_atom(false, b"WM_DELETE_WINDOW")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    // El message_type es deliberadamente distinto de WM_PROTOCOLS: este mensaje
    // no es una petición de cierre y Baud no debe interpretarlo como tal.
    let bogus = conn
        .intern_atom(false, b"_BAUD_NOT_WM_PROTOCOLS")
        .unwrap()
        .reply()
        .unwrap()
        .atom;

    let event = ClientMessageEvent::new(32, win, bogus, [delete, 0, 0, 0, 0]);
    conn.send_event(false, win, EventMask::NO_EVENT, event)
        .unwrap();
    conn.sync().unwrap();

    std::thread::sleep(Duration::from_secs(2));

    let estado = child.try_wait().expect("no se pudo consultar el proceso");
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        estado.is_none(),
        "baud murió ({estado:?}) tras un ClientMessage que no era WM_PROTOCOLS"
    );
}
