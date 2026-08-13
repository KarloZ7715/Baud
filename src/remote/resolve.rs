//! Resolucion de peticiones de control contra el estado de `App`.
//!
//! `send_*` no escribe al PTY: devuelve los bytes para que el event loop
//! los envie. `wait_for` solo se cierra aqui si el patron ya esta; si no,
//! el event loop guarda la espera y la reevalúa tras cada drain.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ansi::{Color, Term};
use crate::input::actions::parse_binding;
use crate::input::keymap::{self, KeyModes};
use crate::session::SessionId;
use crate::window::App;

use super::{Request, Response};

/// Espera `wait_for` que el event loop reevalúa tras output o al vencer.
pub struct PendingWait {
    pub id: u64,
    pub session: Option<u64>,
    pub pattern: String,
    pub deadline: Instant,
    pub tx: std::sync::mpsc::Sender<Response>,
}

/// Resultado de resolver una peticion sin efectos de I/O.
pub enum ResolveOutcome {
    Done(Response),
    Wait {
        id: u64,
        session: Option<u64>,
        pattern: String,
        timeout_ms: u64,
    },
}

impl ResolveOutcome {
    fn done(r: Response) -> Self {
        Self::Done(r)
    }
}

pub fn resolve_request(app: &App, req: &Request) -> ResolveOutcome {
    match req.method.as_str() {
        "hello" => ResolveOutcome::done(Response::err(
            req.id,
            "unexpected_method",
            "hello is handled at the connection layer",
        )),
        "list_sessions" => ResolveOutcome::done(list_sessions(app, req.id)),
        "screen_text" => ResolveOutcome::done(screen_text(app, req)),
        "screen_detail" => ResolveOutcome::done(screen_detail(app, req)),
        "send_text" => ResolveOutcome::done(send_text(app, req)),
        "send_key" => ResolveOutcome::done(send_key(app, req)),
        "wait_for" => wait_for(app, req),
        "get_config" => ResolveOutcome::done(get_config(app, req.id)),
        "tail_log" => ResolveOutcome::done(tail_log(req)),
        other => ResolveOutcome::done(Response::err(
            req.id,
            "unknown_method",
            format!("unknown method '{other}'"),
        )),
    }
}

/// True si la pantalla de la sesion (visible + scrollback reciente) contiene `pattern`.
pub fn screen_contains(app: &App, session: Option<u64>, pattern: &str) -> bool {
    let Ok(term) = lock_session_term(app, session, 0) else {
        return false;
    };
    term.visible_text_lines(term.grid.scrollback.len())
        .join("\n")
        .contains(pattern)
}

fn list_sessions(app: &App, id: u64) -> Response {
    let focused_tab = app.focused_tab();
    let tabs: Vec<Value> = app
        .tabs()
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let focused_id = tab.focused();
            let panes: Vec<Value> = tab
                .leaves()
                .into_iter()
                .map(|sid| {
                    let title = app
                        .session_by_id(sid)
                        .map(|i| app.sessions()[i].session.title.clone())
                        .unwrap_or_default();
                    json!({
                        "id": sid.0,
                        "title": title,
                        "focused": sid == focused_id,
                    })
                })
                .collect();
            json!({
                "index": index,
                "focused": index == focused_tab,
                "panes": panes,
            })
        })
        .collect();
    Response::ok(id, json!({ "tabs": tabs }))
}

fn screen_text(app: &App, req: &Request) -> Response {
    let session = match optional_session(&req.params, req.id) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let extra = req
        .params
        .get("scrollback_lines")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let term = match lock_session_term(app, session, req.id) {
        Ok(t) => t,
        Err(r) => return r,
    };
    let lines = term.visible_text_lines(extra);
    Response::ok(req.id, json!({ "lines": lines }))
}

fn screen_detail(app: &App, req: &Request) -> Response {
    let session = match optional_session(&req.params, req.id) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let term = match lock_session_term(app, session, req.id) {
        Ok(t) => t,
        Err(r) => return r,
    };
    let grid = term.active_grid();
    let rows = grid.rows_count;
    let cols = grid.cols_count;
    let (x, y, w, h) = parse_rect(&req.params, cols, rows);
    let mut cells = Vec::with_capacity(w.saturating_mul(h));
    for row in y..y.saturating_add(h).min(rows) {
        let Some(line) = grid.rows.get(row) else {
            continue;
        };
        for (col, cell) in line
            .iter()
            .enumerate()
            .take(x.saturating_add(w).min(cols).min(line.len()))
            .skip(x)
        {
            cells.push(json!({
                "row": row,
                "col": col,
                "ch": cell.ch.to_string(),
                "width": cell.width(),
                "fg": color_json(cell.attrs.fg()),
                "bg": color_json(cell.attrs.bg()),
                "bold": cell.attrs.bold(),
                "italic": cell.attrs.italic(),
                "underline": cell.attrs.underline(),
                "dim": cell.attrs.dim(),
                "reverse": cell.attrs.reverse(),
                "strikethrough": cell.attrs.strikethrough(),
            }));
        }
    }
    Response::ok(
        req.id,
        json!({
            "cols": cols,
            "rows": rows,
            "cells": cells,
            "cursor": {
                "row": term.cursor.row,
                "col": term.cursor.col,
                "visible": term.cursor_visible,
            },
            "modes": {
                "alt_screen": term.alt_screen,
                "mouse": term.mouse_reporting.is_active(),
                "extended_keyboard": term.keyboard_flags != 0,
                "bracketed_paste": term.bracketed_paste,
            },
        }),
    )
}

fn send_text(app: &App, req: &Request) -> Response {
    let session = match optional_session(&req.params, req.id) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(text) = req.params.get("text").and_then(Value::as_str) else {
        return Response::err(req.id, "invalid_params", "missing 'text'");
    };
    let sid = match resolve_session_id(app, session, req.id) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let bytes = crate::input::paste_text(text);
    if bytes.is_empty() {
        return Response::ok(req.id, json!({ "written": 0 }));
    }
    let n = bytes.len();
    Response::ok_write(req.id, json!({ "written": n }), sid.0, bytes)
}

fn send_key(app: &App, req: &Request) -> Response {
    let session = match optional_session(&req.params, req.id) {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(chord) = req.params.get("chord").and_then(Value::as_str) else {
        return Response::err(req.id, "invalid_params", "missing 'chord'");
    };
    let Some((key, mods)) = parse_binding(chord) else {
        return Response::err(
            req.id,
            "invalid_chord",
            format!("cannot parse chord '{chord}'"),
        );
    };
    let sid = match resolve_session_id(app, session, req.id) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let modes = match lock_session_term(app, session, req.id) {
        Ok(term) => KeyModes {
            app_cursor_keys: term.app_cursor_keys,
            app_keypad: term.keypad_application_mode,
            newline_mode: term.newline_mode,
            keyboard_flags: term.keyboard_flags,
        },
        Err(r) => return r,
    };
    let Some(bytes) = keymap::encode_key(key, mods, modes) else {
        return Response::err(
            req.id,
            "unencodable",
            format!("chord '{chord}' has no encoding"),
        );
    };
    Response::ok_write(req.id, json!({ "written": bytes.len() }), sid.0, bytes)
}

fn wait_for(app: &App, req: &Request) -> ResolveOutcome {
    let session = match optional_session(&req.params, req.id) {
        Ok(s) => s,
        Err(r) => return ResolveOutcome::done(r),
    };
    let Some(pattern) = req.params.get("pattern").and_then(Value::as_str) else {
        return ResolveOutcome::done(Response::err(req.id, "invalid_params", "missing 'pattern'"));
    };
    if pattern.is_empty() {
        return ResolveOutcome::done(Response::err(
            req.id,
            "invalid_params",
            "pattern must not be empty",
        ));
    };
    if lock_session_term(app, session, req.id).is_err() {
        return ResolveOutcome::done(Response::err(req.id, "no_session", "session not found"));
    }
    if screen_contains(app, session, pattern) {
        return ResolveOutcome::done(Response::ok(req.id, json!({ "matched": true })));
    }
    let timeout_ms = req
        .params
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(5_000);
    ResolveOutcome::Wait {
        id: req.id,
        session,
        pattern: pattern.to_string(),
        timeout_ms,
    }
}

fn get_config(app: &App, id: u64) -> Response {
    match serde_json::to_value(app.config()) {
        Ok(body) => Response::ok(id, body),
        Err(e) => Response::err(id, "serialize", e.to_string()),
    }
}

fn tail_log(req: &Request) -> Response {
    let lines = req
        .params
        .get("lines")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .min(1_000) as usize;
    let dir = crate::diagnostics::logging::log_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Response::ok(req.id, json!({ "lines": [] }));
    };
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("baud.log"))
        .collect();
    files.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    let Some(latest) = files.last() else {
        return Response::ok(req.id, json!({ "lines": [] }));
    };
    let Ok(content) = std::fs::read_to_string(latest.path()) else {
        return Response::ok(req.id, json!({ "lines": [] }));
    };
    let mut all: Vec<String> = content.lines().map(str::to_string).collect();
    if all.len() > lines {
        all = all.split_off(all.len() - lines);
    }
    Response::ok(req.id, json!({ "lines": all }))
}

fn optional_session(params: &Value, id: u64) -> Result<Option<u64>, Response> {
    match params.get("session") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .ok_or_else(|| Response::err(id, "invalid_params", "'session' must be an integer"))
            .map(Some),
    }
}

fn resolve_session_id(app: &App, session: Option<u64>, req_id: u64) -> Result<SessionId, Response> {
    match session {
        None => Ok(app.focused_session().id),
        Some(raw) => {
            let sid = SessionId(raw);
            if app.session_by_id(sid).is_some() {
                Ok(sid)
            } else {
                Err(Response::err(
                    req_id,
                    "no_session",
                    format!("session {raw} not found"),
                ))
            }
        }
    }
}

fn lock_session_term(
    app: &App,
    session: Option<u64>,
    req_id: u64,
) -> Result<std::sync::MutexGuard<'_, Term>, Response> {
    let sid = resolve_session_id(app, session, req_id)?;
    let idx = app
        .session_by_id(sid)
        .ok_or_else(|| Response::err(req_id, "no_session", "session not found"))?;
    match app.sessions()[idx].session.term.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => Ok(poisoned.into_inner()),
    }
}

fn parse_rect(params: &Value, cols: usize, rows: usize) -> (usize, usize, usize, usize) {
    let Some(rect) = params.get("rect") else {
        return (0, 0, cols, rows);
    };
    let x = rect.get("x").and_then(Value::as_u64).unwrap_or(0) as usize;
    let y = rect.get("y").and_then(Value::as_u64).unwrap_or(0) as usize;
    let w = rect
        .get("cols")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(cols.saturating_sub(x));
    let h = rect
        .get("rows")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(rows.saturating_sub(y));
    (x.min(cols), y.min(rows), w, h)
}

fn color_json(color: Color) -> Value {
    match color {
        Color::Default => json!("default"),
        Color::Black => json!("black"),
        Color::Red => json!("red"),
        Color::Green => json!("green"),
        Color::Yellow => json!("yellow"),
        Color::Blue => json!("blue"),
        Color::Magenta => json!("magenta"),
        Color::Cyan => json!("cyan"),
        Color::White => json!("white"),
        Color::BrightBlack => json!("bright_black"),
        Color::BrightRed => json!("bright_red"),
        Color::BrightGreen => json!("bright_green"),
        Color::BrightYellow => json!("bright_yellow"),
        Color::BrightBlue => json!("bright_blue"),
        Color::BrightMagenta => json!("bright_magenta"),
        Color::BrightCyan => json!("bright_cyan"),
        Color::BrightWhite => json!("bright_white"),
        Color::Indexed(i) => json!({ "indexed": i }),
        Color::Rgb(r, g, b) => json!({ "rgb": [r, g, b] }),
    }
}

/// Construye el `PendingWait` que el event loop guarda cuando el patron aun no esta.
pub fn pending_from_wait(
    id: u64,
    session: Option<u64>,
    pattern: String,
    timeout_ms: u64,
    tx: std::sync::mpsc::Sender<Response>,
) -> PendingWait {
    PendingWait {
        id,
        session,
        pattern,
        deadline: Instant::now() + Duration::from_millis(timeout_ms),
        tx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::watch::WatchState;
    use crate::config::{Config, ConfigSource};
    use crate::event_loop::BlinkFocus;
    use crate::pty::PtyCommandSender;
    use crate::session::Session;
    use crate::watchdog::EventLoopWatchdog;
    use crate::window::SessionHost;
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc, Mutex};

    fn dummy_pty_sender() -> PtyCommandSender {
        let (tx, _rx) = mpsc::channel();
        let wakeup = crate::pty::create_wake().expect("wake para test");
        PtyCommandSender::new_for_test(tx, wakeup)
    }

    fn test_session(term: Arc<Mutex<Term>>) -> Session {
        Session {
            id: SessionId::next(),
            term,
            pty_tx: dummy_pty_sender(),
            title: String::new(),
            dirty: false,
            hold: false,
            close_on_exit: false,
            has_activity: false,
            foreground_probe: None,
            foreground_cache: None,
            input_reset_pending: Arc::new(AtomicBool::new(false)),
            echo_pending: Arc::new(AtomicBool::new(false)),
        }
    }

    fn test_app(term: Arc<Mutex<Term>>) -> App {
        let session = test_session(term);
        let id = session.id;
        App::new(
            vec![SessionHost::test(session)],
            Config::default(),
            Arc::new(Mutex::new(WatchState::new(None))),
            None,
            BlinkFocus::new(id),
            ConfigSource::Ok,
            EventLoopWatchdog::noop(),
            None,
            None,
        )
    }

    fn feed(term: &mut Term, data: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(term, data);
    }

    fn test_app_con_texto(text: &str) -> App {
        let mut term = Term::new();
        feed(&mut term, text.as_bytes());
        test_app(Arc::new(Mutex::new(term)))
    }

    fn done(outcome: ResolveOutcome) -> Response {
        match outcome {
            ResolveOutcome::Done(r) => r,
            ResolveOutcome::Wait { .. } => panic!("esperaba respuesta inmediata"),
        }
    }

    #[test]
    fn screen_text_devuelve_lo_visible() {
        let app = test_app_con_texto("hola\nmundo");
        let r = done(resolve_request(&app, &Request::screen_text_default()));
        assert!(r.is_ok());
        assert!(r.text_lines().unwrap().join("\n").contains("hola"));
    }

    #[test]
    fn send_key_encodea_el_chord() {
        let app = test_app(Arc::new(Mutex::new(Term::new())));
        let r = done(resolve_request(&app, &Request::send_key("ctrl+c")));
        assert_eq!(r.bytes_to_write().unwrap(), b"\x03");
        let r = done(resolve_request(&app, &Request::send_key("ctrl+d")));
        assert_eq!(r.bytes_to_write().unwrap(), b"\x04");
    }

    #[test]
    fn chord_invalido_es_err_no_panic() {
        let app = test_app(Arc::new(Mutex::new(Term::new())));
        let r = done(resolve_request(&app, &Request::send_key("mega+tecla")));
        assert!(matches!(r, Response::Err { .. }));
    }

    #[test]
    fn screen_detail_incluye_modos_y_cursor() {
        let mut term = Term::new();
        feed(&mut term, b"\x1b[?1049h");
        let app = test_app(Arc::new(Mutex::new(term)));
        let req = Request {
            id: 1,
            method: "screen_detail".into(),
            params: json!({}),
        };
        let r = done(resolve_request(&app, &req));
        let Response::Ok { body, .. } = r else {
            panic!("esperaba ok");
        };
        assert_eq!(body["modes"]["alt_screen"], true);
        assert!(body["cursor"]["row"].is_number());
        assert!(body["cursor"]["col"].is_number());
    }

    #[test]
    fn wait_for_inmediato_si_el_patron_ya_esta() {
        let app = test_app_con_texto("hola");
        let r = done(resolve_request(&app, &Request::wait_for("hola", 1_000)));
        assert!(r.is_ok());
    }

    #[test]
    fn wait_for_pendiente_si_el_patron_no_esta() {
        let app = test_app(Arc::new(Mutex::new(Term::new())));
        match resolve_request(&app, &Request::wait_for("hola", 1_000)) {
            ResolveOutcome::Wait { pattern, .. } => assert_eq!(pattern, "hola"),
            ResolveOutcome::Done(_) => panic!("debia quedar pendiente"),
        }
    }

    #[test]
    fn wait_for_feed_posterior_resuelve_la_espera() {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut app = test_app(Arc::clone(&term));
        let id = app.focused_session().id;
        let (tx, rx) = mpsc::channel();
        app.dispatch_user_event(crate::window::UserEvent::Remote(
            Request::wait_for("hola", 5_000),
            tx,
        ));
        assert!(rx.try_recv().is_err(), "aun no debe haber respuesta");
        {
            let mut t = term.lock().unwrap();
            feed(&mut t, b"hola");
        }
        app.dispatch_user_event(crate::window::UserEvent::RedrawNeeded(id));
        let r = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(r.is_ok());
    }

    #[test]
    fn wait_for_timeout_responde_err() {
        let mut app = test_app(Arc::new(Mutex::new(Term::new())));
        let (tx, rx) = mpsc::channel();
        app.dispatch_user_event(crate::window::UserEvent::Remote(
            Request::wait_for("hola", 0),
            tx,
        ));
        let r = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(r, Response::Err { ref code, .. } if code == "timeout"));
    }
}
