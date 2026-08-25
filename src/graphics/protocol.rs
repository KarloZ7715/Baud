//! Gramática APC `G` del protocolo de gráficos: pares `k=v`, chunks y respuestas.

use super::{GraphicsResponse, ImageId};

/// Acción de un comando de gráficos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TransmitDisplay,
    Transmit,
    Put,
    Delete,
    Query,
}

/// Claves opcionales del bloque de control.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keys {
    pub format: Option<u32>,
    pub transmission: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub data_size: Option<u32>,
    pub data_offset: Option<u32>,
    pub image_id: Option<u32>,
    pub image_number: Option<u32>,
    pub placement_id: Option<u32>,
    pub compression: Option<u8>,
    pub more: Option<bool>,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub src_x: Option<u32>,
    pub src_y: Option<u32>,
    pub src_w: Option<u32>,
    pub src_h: Option<u32>,
    pub z: Option<i32>,
    pub quiet: Option<u8>,
    pub delete: Option<u8>,
    pub no_cursor_move: Option<bool>,
}

/// Comando ya parseado, con payload en binario (base64 decodificado).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsCommand {
    pub action: Action,
    pub keys: Keys,
    pub payload: Vec<u8>,
}

impl GraphicsCommand {
    pub fn quiet(&self) -> u8 {
        self.keys.quiet.unwrap_or(0)
    }

    pub fn image_id(&self) -> ImageId {
        ImageId(self.keys.image_id.unwrap_or(0))
    }
}

/// Error de gramática o de payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsError {
    Payload,
    Invalid,
}

impl GraphicsError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Payload | Self::Invalid => "EINVAL",
        }
    }
}

/// Acumula fragmentos `m=1` hasta el cierre `m=0`.
#[derive(Debug, Default, Clone)]
pub struct ChunkAssembler {
    pending: Option<GraphicsCommand>,
}

impl ChunkAssembler {
    pub fn push(&mut self, mut cmd: GraphicsCommand) -> Option<GraphicsCommand> {
        let more = cmd.keys.more.unwrap_or(false);
        if more {
            if let Some(pending) = self.pending.as_mut() {
                pending.payload.append(&mut cmd.payload);
                if let Some(q) = cmd.keys.quiet {
                    pending.keys.quiet = Some(q);
                }
            } else {
                self.pending = Some(cmd);
            }
            None
        } else if let Some(mut pending) = self.pending.take() {
            pending.payload.append(&mut cmd.payload);
            if let Some(q) = cmd.keys.quiet {
                pending.keys.quiet = Some(q);
            }
            Some(pending)
        } else {
            Some(cmd)
        }
    }

    pub fn abort(&mut self) {
        self.pending = None;
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_none()
    }
}

impl GraphicsResponse {
    pub fn ok(id: u32) -> Self {
        Self {
            image_id: ImageId(id),
            image_number: 0,
            placement_id: 0,
            error: None,
        }
    }

    pub fn err(id: u32, code: impl Into<String>) -> Self {
        Self {
            image_id: ImageId(id),
            image_number: 0,
            placement_id: 0,
            error: Some(code.into()),
        }
    }

    /// Codifica la respuesta APC. `None` si `q` suprime este tipo de mensaje.
    pub fn encode(&self, quiet: u8) -> Option<Vec<u8>> {
        let is_ok = self.error.is_none();
        if quiet >= 2 || (quiet == 1 && is_ok) {
            return None;
        }
        let mut out = Vec::with_capacity(32);
        out.extend_from_slice(b"\x1b_G");
        let mut first = true;
        append_key(&mut out, &mut first, b"i=", self.image_id.0);
        append_key(&mut out, &mut first, b"I=", self.image_number);
        append_key(&mut out, &mut first, b"p=", self.placement_id);
        out.push(b';');
        if let Some(err) = &self.error {
            out.extend_from_slice(err.as_bytes());
        } else {
            out.extend_from_slice(b"OK");
        }
        out.extend_from_slice(b"\x1b\\");
        Some(out)
    }
}

fn append_key(out: &mut Vec<u8>, first: &mut bool, prefix: &[u8], value: u32) {
    if value == 0 {
        return;
    }
    if !*first {
        out.push(b',');
    }
    *first = false;
    out.extend_from_slice(prefix);
    out.extend_from_slice(value.to_string().as_bytes());
}

/// Parsea el cuerpo de una APC que empieza por `G`.
pub fn parse(apc: &[u8]) -> Result<GraphicsCommand, GraphicsError> {
    let body = match apc.first() {
        Some(b'G') => &apc[1..],
        _ => return Err(GraphicsError::Invalid),
    };
    let (ctrl, payload_b64) = match body.iter().position(|&b| b == b';') {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => (body, &[][..]),
    };

    let mut keys = Keys::default();
    let mut action = Action::Transmit;
    if !ctrl.is_empty() {
        for pair in ctrl.split(|&b| b == b',') {
            if pair.is_empty() {
                continue;
            }
            let Some(eq) = pair.iter().position(|&b| b == b'=') else {
                return Err(GraphicsError::Invalid);
            };
            let k = &pair[..eq];
            let v = &pair[eq + 1..];
            match k {
                b"a" => action = parse_action(v)?,
                b"f" => keys.format = Some(parse_u32(v)?),
                b"t" => keys.transmission = Some(parse_byte(v)?),
                b"s" => keys.width = Some(parse_u32(v)?),
                b"v" => keys.height = Some(parse_u32(v)?),
                b"S" => keys.data_size = Some(parse_u32(v)?),
                b"O" => keys.data_offset = Some(parse_u32(v)?),
                b"i" => keys.image_id = Some(parse_u32(v)?),
                b"I" => keys.image_number = Some(parse_u32(v)?),
                b"p" => keys.placement_id = Some(parse_u32(v)?),
                b"o" => keys.compression = Some(parse_byte(v)?),
                b"m" => keys.more = Some(parse_flag(v)?),
                b"c" => keys.columns = Some(parse_u32(v)?),
                b"r" => keys.rows = Some(parse_u32(v)?),
                b"x" => keys.src_x = Some(parse_u32(v)?),
                b"y" => keys.src_y = Some(parse_u32(v)?),
                b"w" => keys.src_w = Some(parse_u32(v)?),
                b"h" => keys.src_h = Some(parse_u32(v)?),
                b"z" => keys.z = Some(parse_i32(v)?),
                b"q" => keys.quiet = Some(parse_u32(v)? as u8),
                b"d" => keys.delete = Some(parse_byte(v)?),
                b"C" => keys.no_cursor_move = Some(parse_flag(v)?),
                _ => {}
            }
        }
    }

    if keys.image_id.is_some() && keys.image_number.is_some() {
        return Err(GraphicsError::Invalid);
    }

    let payload = if payload_b64.is_empty() {
        Vec::new()
    } else {
        crate::base64::decode(payload_b64).ok_or(GraphicsError::Payload)?
    };

    Ok(GraphicsCommand {
        action,
        keys,
        payload,
    })
}

fn parse_action(v: &[u8]) -> Result<Action, GraphicsError> {
    match v {
        b"T" => Ok(Action::TransmitDisplay),
        b"t" => Ok(Action::Transmit),
        b"p" => Ok(Action::Put),
        b"d" => Ok(Action::Delete),
        b"q" => Ok(Action::Query),
        _ => Err(GraphicsError::Invalid),
    }
}

fn parse_u32(v: &[u8]) -> Result<u32, GraphicsError> {
    let s = std::str::from_utf8(v).map_err(|_| GraphicsError::Invalid)?;
    s.parse::<u32>().map_err(|_| GraphicsError::Invalid)
}

fn parse_i32(v: &[u8]) -> Result<i32, GraphicsError> {
    let s = std::str::from_utf8(v).map_err(|_| GraphicsError::Invalid)?;
    s.parse().map_err(|_| GraphicsError::Invalid)
}

fn parse_byte(v: &[u8]) -> Result<u8, GraphicsError> {
    match v {
        [b] if b.is_ascii() => Ok(*b),
        _ => Err(GraphicsError::Invalid),
    }
}

fn parse_flag(v: &[u8]) -> Result<bool, GraphicsError> {
    match v {
        b"0" => Ok(false),
        b"1" => Ok(true),
        _ => Err(GraphicsError::Invalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_transmit_display_minimo() {
        let cmd = parse(b"Ga=T,f=100,s=10,v=10;QUJD").unwrap();
        assert!(matches!(cmd.action, Action::TransmitDisplay));
        assert_eq!(cmd.keys.format, Some(100));
        assert_eq!(cmd.payload, b"ABC");
    }

    #[test]
    fn clave_desconocida_se_ignora_no_falla() {
        assert!(parse(b"Ga=T,f=100,zz=9;QUJD").is_ok());
    }

    #[test]
    fn payload_base64_corrupto_es_error() {
        assert!(matches!(parse(b"Ga=T;@@@@"), Err(GraphicsError::Payload)));
    }

    #[test]
    fn chunks_m1_acumulan_hasta_m0() {
        let mut asm = ChunkAssembler::default();
        assert!(asm.push(parse(b"Ga=T,f=100,m=1;QUJD").unwrap()).is_none());
        let full = asm.push(parse(b"Gm=0;REVG").unwrap()).unwrap();
        assert_eq!(full.payload, b"ABCDEF");
        assert!(matches!(full.action, Action::TransmitDisplay));
        assert_eq!(full.keys.format, Some(100));
    }

    #[test]
    fn respuesta_ok_y_quiet() {
        let r = GraphicsResponse::ok(31);
        assert_eq!(r.encode(0).unwrap(), b"\x1b_Gi=31;OK\x1b\\".to_vec());
        assert!(r.encode(1).is_none());
    }

    #[test]
    fn respuesta_error_no_se_suprime_con_q1() {
        let r = GraphicsResponse::err(31, "EINVAL");
        assert_eq!(r.encode(1).unwrap(), b"\x1b_Gi=31;EINVAL\x1b\\".to_vec());
        assert!(r.encode(2).is_none());
    }
}
