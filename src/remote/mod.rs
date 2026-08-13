//! Control remoto local: protocolo JSON Lines, servidor IPC y adaptador MCP.
//!
//! El hilo IPC nunca toca `Term`. Empaqueta cada peticion como
//! `UserEvent::Remote` y el event loop responde bajo el mismo lock que el render.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod resolve;
pub mod server;

pub use resolve::{
    pending_from_wait, resolve_request, screen_contains, PendingWait, ResolveOutcome,
};

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

/// Peticion del protocolo v1: una linea JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl Request {
    pub fn screen_text_default() -> Self {
        Self {
            id: 1,
            method: "screen_text".into(),
            params: serde_json::json!({}),
        }
    }

    pub fn send_key(chord: &str) -> Self {
        Self {
            id: 1,
            method: "send_key".into(),
            params: serde_json::json!({ "chord": chord }),
        }
    }

    pub fn wait_for(pattern: &str, timeout_ms: u64) -> Self {
        Self {
            id: 1,
            method: "wait_for".into(),
            params: serde_json::json!({ "pattern": pattern, "timeout_ms": timeout_ms }),
        }
    }
}

/// Respuesta del protocolo v1. `write` no viaja por el cable: el event loop
/// lo consume para enviar bytes al PTY.
#[derive(Debug, Clone)]
pub enum Response {
    Ok {
        id: u64,
        body: serde_json::Value,
        write: Option<(u64, Vec<u8>)>,
    },
    Err {
        id: u64,
        code: String,
        msg: String,
    },
}

impl Response {
    pub fn ok(id: u64, body: serde_json::Value) -> Self {
        Self::Ok {
            id,
            body,
            write: None,
        }
    }

    pub fn ok_write(id: u64, body: serde_json::Value, session: u64, bytes: Vec<u8>) -> Self {
        Self::Ok {
            id,
            body,
            write: Some((session, bytes)),
        }
    }

    pub fn err(id: u64, code: &str, msg: impl Into<String>) -> Self {
        Self::Err {
            id,
            code: code.to_string(),
            msg: msg.into(),
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    pub fn text_lines(&self) -> Option<Vec<String>> {
        match self {
            Self::Ok { body, .. } => body.get("lines").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            }),
            Self::Err { .. } => None,
        }
    }

    pub fn bytes_to_write(&self) -> Option<&[u8]> {
        match self {
            Self::Ok {
                write: Some((_, bytes)),
                ..
            } => Some(bytes.as_slice()),
            _ => None,
        }
    }

    pub fn take_write(&mut self) -> Option<(u64, Vec<u8>)> {
        match self {
            Self::Ok { write, .. } => write.take(),
            Self::Err { .. } => None,
        }
    }

    pub fn from_json_line(line: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("invalid json: {e}"))?;
        let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
        if let Some(ok) = v.get("ok") {
            return Ok(Self::ok(id, ok.clone()));
        }
        if let Some(err) = v.get("err") {
            let code = err.get("code").and_then(|x| x.as_str()).unwrap_or("error");
            let msg = err.get("msg").and_then(|x| x.as_str()).unwrap_or("");
            return Ok(Self::err(id, code, msg));
        }
        Err("response missing ok or err".into())
    }

    pub fn id(&self) -> u64 {
        match self {
            Self::Ok { id, .. } | Self::Err { id, .. } => *id,
        }
    }
}

impl Serialize for Response {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Ok { id, body, .. } => {
                let mut s = serializer.serialize_struct("Response", 2)?;
                s.serialize_field("id", id)?;
                s.serialize_field("ok", body)?;
                s.end()
            }
            Self::Err { id, code, msg } => {
                let mut s = serializer.serialize_struct("Response", 2)?;
                s.serialize_field("id", id)?;
                s.serialize_field("err", &ErrorBody { code, msg })?;
                s.end()
            }
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    msg: &'a str,
}
