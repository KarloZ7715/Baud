//! Codificacion/decodificacion base64 estandar (sin dependencia externa).

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn decode_char(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Codifica bytes a base64 ASCII.
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 63) as usize] as char);
        out.push(TABLE[((triple >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decodifica base64. `None` si el input es invalido.
pub fn decode(input: &[u8]) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = input
        .iter()
        .copied()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut vals = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                vals[i] = 0;
            } else {
                vals[i] = decode_char(c)?;
            }
        }
        let triple = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | (vals[3] as u32);
        out.push((triple >> 16) as u8);
        if chunk.get(2).is_some_and(|&c| c != b'=') {
            out.push((triple >> 8) as u8);
        }
        if chunk.get(3).is_some_and(|&c| c != b'=') {
            out.push(triple as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_roundtrip() {
        for s in ["", "h", "hi", "hola", "any+slash/=="] {
            let enc = encode(s.as_bytes());
            let dec = decode(enc.as_bytes()).unwrap();
            assert_eq!(dec, s.as_bytes());
        }
    }

    #[test]
    fn test_base64_decode_invalido_es_none() {
        assert!(decode(b"!!!!").is_none());
    }

    #[test]
    fn test_base64_vectores_conocidos() {
        assert_eq!(encode(b"hello"), "aGVsbG8=");
        assert_eq!(decode(b"aGVsbG8=").unwrap(), b"hello");
    }
}
