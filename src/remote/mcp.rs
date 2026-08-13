//! Adaptador MCP: JSON-RPC 2.0 por stdio hacia el protocolo de control remoto.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::cli::EXIT_ERR;
use crate::cli::EXIT_OK;

use super::server::{self, Client};
use super::{Request, Response};

const NO_INSTANCE: &str = "no running baud instance with remote_control enabled";

/// Punto de entrada de `baud mcp [--socket <ruta>]`.
pub fn run(socket: Option<&str>) -> i32 {
    match run_inner(socket) {
        Ok(()) => EXIT_OK,
        Err(msg) => {
            eprintln!("{msg}");
            EXIT_ERR
        }
    }
}

fn run_inner(socket: Option<&str>) -> Result<(), String> {
    let (target, token) = resolve_endpoint(socket)?;
    let mut client = Client::connect(&target, &token).map_err(|_| NO_INSTANCE.to_string())?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(
                    stdout,
                    "{}",
                    jsonrpc_error(Value::Null, -32700, &format!("parse error: {e}"))
                );
                let _ = stdout.flush();
                continue;
            }
        };
        if let Some(reply) = handle_message(&mut client, &msg) {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string(&reply).unwrap_or_else(|_| "{}".into())
            );
            let _ = stdout.flush();
        }
    }
    Ok(())
}

fn resolve_endpoint(socket: Option<&str>) -> Result<(String, String), String> {
    if let Some(path) = socket {
        return endpoint_from_socket(path);
    }
    match server::discover() {
        Ok(Some(pair)) => Ok(pair),
        Ok(None) | Err(_) => Err(NO_INSTANCE.to_string()),
    }
}

fn endpoint_from_socket(path: &str) -> Result<(String, String), String> {
    #[cfg(unix)]
    {
        use std::path::Path;
        let p = Path::new(path);
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let token_path = p.with_file_name(format!("{stem}.token"));
        let token = std::fs::read_to_string(&token_path)
            .map_err(|_| NO_INSTANCE.to_string())?
            .trim()
            .to_string();
        if token.is_empty() {
            return Err(NO_INSTANCE.to_string());
        }
        Ok((path.to_string(), token))
    }
    #[cfg(windows)]
    {
        if let Ok(Some((target, token))) = server::discover() {
            if target == path {
                return Ok((target, token));
            }
        }
        Err(NO_INSTANCE.to_string())
    }
}

/// Procesa un mensaje JSON-RPC. `None` = notificacion, no hay respuesta.
pub fn handle_message(client: &mut Client, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = msg.get("id").cloned()?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match method {
        "initialize" => jsonrpc_ok(id, initialize_result()),
        "tools/list" => jsonrpc_ok(id, tools_list_value()),
        "tools/call" => match tools_call(client, &params) {
            Ok(body) => jsonrpc_ok(id, body),
            Err(e) => jsonrpc_error(id, -32602, &e),
        },
        "ping" => jsonrpc_ok(id, json!({})),
        _ => jsonrpc_error(id, -32601, "Method not found"),
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "baud",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn tool_defs() -> Vec<Value> {
    vec![
        tool(
            "baud_list_sessions",
            "List open tabs and panes: ids, titles, and which one has focus. Call this first when you need a session id for the other tools.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "baud_screen",
            "Read the visible screen as plain text lines. Use this to see what is currently displayed. Optional scrollback_lines includes history above the viewport.",
            json!({
                "type": "object",
                "properties": {
                    "session": { "type": "integer", "description": "Session id from baud_list_sessions. Omit for the focused pane." },
                    "scrollback_lines": { "type": "integer", "description": "Extra history lines above the visible screen. Default 0." }
                }
            }),
        ),
        tool(
            "baud_screen_detail",
            "Read cells with colors and attributes, plus cursor position and active modes (alt screen, mouse reporting, extended keyboard, bracketed paste). Use when plain text is not enough to audit the display.",
            json!({
                "type": "object",
                "properties": {
                    "session": { "type": "integer", "description": "Session id from baud_list_sessions. Omit for the focused pane." },
                    "rect": {
                        "type": "object",
                        "description": "Optional cell rectangle {x, y, cols, rows}. Default is the full visible grid.",
                        "properties": {
                            "x": { "type": "integer" },
                            "y": { "type": "integer" },
                            "cols": { "type": "integer" },
                            "rows": { "type": "integer" }
                        }
                    }
                }
            }),
        ),
        tool(
            "baud_send_text",
            "Write text to the PTY as a paste without bracketed-paste markers. Use for commands or input. Include a trailing newline if the program should receive Enter.",
            json!({
                "type": "object",
                "properties": {
                    "session": { "type": "integer", "description": "Session id from baud_list_sessions. Omit for the focused pane." },
                    "text": { "type": "string", "description": "Bytes to write." }
                },
                "required": ["text"]
            }),
        ),
        tool(
            "baud_send_key",
            "Encode a key chord (the same grammar as config keybindings, e.g. ctrl+c, enter, alt+up) and write it to the PTY.",
            json!({
                "type": "object",
                "properties": {
                    "session": { "type": "integer", "description": "Session id from baud_list_sessions. Omit for the focused pane." },
                    "chord": { "type": "string", "description": "Chord such as ctrl+c or enter." }
                },
                "required": ["chord"]
            }),
        ),
        tool(
            "baud_wait_for",
            "Block until the screen contains a substring, or until timeout_ms elapses. Use after baud_send_text to wait for output.",
            json!({
                "type": "object",
                "properties": {
                    "session": { "type": "integer", "description": "Session id from baud_list_sessions. Omit for the focused pane." },
                    "pattern": { "type": "string", "description": "Substring to find on the screen." },
                    "timeout_ms": { "type": "integer", "description": "How long to wait. Default 5000." }
                },
                "required": ["pattern"]
            }),
        ),
        tool(
            "baud_get_config",
            "Return the effective configuration as JSON (no secrets). Use to inspect fonts, keybindings, and feature flags.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "baud_tail_log",
            "Return the last N lines of today's Baud log file. Use when diagnosing a crash or unexpected behavior.",
            json!({
                "type": "object",
                "properties": {
                    "lines": { "type": "integer", "description": "How many trailing lines to return. Default 50, max 1000." }
                }
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

fn tools_list_value() -> Value {
    json!({ "tools": tool_defs() })
}

fn tools_call(client: &mut Client, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing tool name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let method = match name {
        "baud_list_sessions" => "list_sessions",
        "baud_screen" => "screen_text",
        "baud_screen_detail" => "screen_detail",
        "baud_send_text" => "send_text",
        "baud_send_key" => "send_key",
        "baud_wait_for" => "wait_for",
        "baud_get_config" => "get_config",
        "baud_tail_log" => "tail_log",
        other => return Err(format!("unknown tool '{other}'")),
    };
    let req = Request {
        id: 1,
        method: method.into(),
        params: args,
    };
    let resp = client.call(req).map_err(|e| e.to_string())?;
    match resp {
        Response::Ok { body, .. } => {
            let text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "isError": false,
            }))
        }
        Response::Err { msg, .. } => Ok(json!({
            "content": [{ "type": "text", "text": msg }],
            "isError": true,
        })),
    }
}

fn jsonrpc_ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::server::{spawn_in, Handler};
    use std::sync::Arc;

    fn stub() -> Handler {
        Arc::new(|req: Request| {
            if req.method == "screen_text" {
                Response::ok(req.id, json!({ "lines": ["hello"] }))
            } else {
                Response::err(req.id, "unknown_method", req.method)
            }
        })
    }

    #[test]
    fn initialize_list_y_call_screen() {
        let dir = tempfile::tempdir().unwrap();
        let server = spawn_in(dir.path(), stub()).unwrap();
        let mut client = Client::connect(server.target(), server.token()).unwrap();

        let init = handle_message(
            &mut client,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "baud");
        assert!(init["result"]["capabilities"]["tools"].is_object());

        let list = handle_message(
            &mut client,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 8);

        let call = handle_message(
            &mut client,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "baud_screen", "arguments": {} }
            }),
        )
        .unwrap();
        assert_eq!(call["result"]["isError"], false);
        let text = call["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("hello"));
    }

    #[test]
    fn metodo_desconocido_es_jsonrpc_error() {
        let dir = tempfile::tempdir().unwrap();
        let server = spawn_in(dir.path(), stub()).unwrap();
        let mut client = Client::connect(server.target(), server.token()).unwrap();
        let reply = handle_message(
            &mut client,
            &json!({"jsonrpc":"2.0","id":1,"method":"nope"}),
        )
        .unwrap();
        assert_eq!(reply["error"]["code"], -32601);
    }
}
