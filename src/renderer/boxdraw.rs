//! Render propio de box-drawing y block elements para alineacion exacta.

/// True si el caracter pertenece a box-drawing o block elements.
pub fn is_box_glyph(ch: char) -> bool {
    matches!(ch as u32, 0x2500..=0x259F)
}

/// True si box_mask puede rasterizar el caracter (sin recurrir a fuente).
pub fn is_box_mask_supported(ch: char) -> bool {
    if !is_box_glyph(ch) {
        return false;
    }
    matches!(
        ch,
        '\u{2500}' | '\u{2502}' | '\u{250C}' | '\u{2510}' | '\u{2514}' | '\u{2518}'
            | '\u{251C}' | '\u{2524}' | '\u{252C}' | '\u{2534}' | '\u{253C}' | '\u{2588}'
            | '\u{2580}' | '\u{2584}' | '\u{258C}' | '\u{2590}' | '\u{2591}' | '\u{2592}'
            | '\u{2593}' | '\u{2581}'..='\u{2587}' | '\u{2589}'..='\u{258F}'
    )
}

/// Grosor de linea en pixeles (al menos 1, ~1/8 de la celda).
fn stroke(w: usize, h: usize) -> usize {
    (w.min(h) / 8).max(1)
}

/// Rellena un rectangulo [x0,x1) x [y0,y1) con 255 en una mascara w*h.
fn fill(mask: &mut [u8], w: usize, x0: usize, y0: usize, x1: usize, y1: usize) {
    for y in y0..y1.min(mask.len() / w) {
        for x in x0..x1.min(w) {
            mask[y * w + x] = 255;
        }
    }
}

/// Mascara alpha para un box/block char. None = no soportado (usar fuente).
pub fn box_mask(ch: char, w: usize, h: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || !is_box_mask_supported(ch) {
        return None;
    }
    let mut m = vec![0u8; w * h];
    let s = stroke(w, h);
    let midx = w / 2;
    let midy = h / 2;
    let left = |m: &mut [u8]| {
        fill(
            m,
            w,
            0,
            midy.saturating_sub(s / 2),
            midx + s,
            midy + s - s / 2,
        )
    };
    let right = |m: &mut [u8]| fill(m, w, midx, midy.saturating_sub(s / 2), w, midy + s - s / 2);
    let up = |m: &mut [u8]| {
        fill(
            m,
            w,
            midx.saturating_sub(s / 2),
            0,
            midx + s - s / 2,
            midy + s,
        )
    };
    let down = |m: &mut [u8]| fill(m, w, midx.saturating_sub(s / 2), midy, midx + s - s / 2, h);

    match ch {
        '\u{2500}' => {
            left(&mut m);
            right(&mut m);
        }
        '\u{2502}' => {
            up(&mut m);
            down(&mut m);
        }
        '\u{250C}' => {
            right(&mut m);
            down(&mut m);
        }
        '\u{2510}' => {
            left(&mut m);
            down(&mut m);
        }
        '\u{2514}' => {
            right(&mut m);
            up(&mut m);
        }
        '\u{2518}' => {
            left(&mut m);
            up(&mut m);
        }
        '\u{251C}' => {
            up(&mut m);
            down(&mut m);
            right(&mut m);
        }
        '\u{2524}' => {
            up(&mut m);
            down(&mut m);
            left(&mut m);
        }
        '\u{252C}' => {
            left(&mut m);
            right(&mut m);
            down(&mut m);
        }
        '\u{2534}' => {
            left(&mut m);
            right(&mut m);
            up(&mut m);
        }
        '\u{253C}' => {
            left(&mut m);
            right(&mut m);
            up(&mut m);
            down(&mut m);
        }
        '\u{2588}' => fill(&mut m, w, 0, 0, w, h),
        '\u{2580}' => fill(&mut m, w, 0, 0, w, midy),
        '\u{2584}' => fill(&mut m, w, 0, midy, w, h),
        '\u{258C}' => fill(&mut m, w, 0, 0, midx, h),
        '\u{2590}' => fill(&mut m, w, midx, 0, w, h),
        '\u{2591}' => m.iter_mut().for_each(|p| *p = 64),
        '\u{2592}' => m.iter_mut().for_each(|p| *p = 128),
        '\u{2593}' => m.iter_mut().for_each(|p| *p = 192),
        c @ '\u{2581}'..='\u{2587}' => {
            let n = (c as u32 - 0x2580) as usize;
            let filled_h = h * n / 8;
            fill(&mut m, w, 0, h - filled_h, w, h);
        }
        c @ '\u{2589}'..='\u{258F}' => {
            let n = 8 - (c as u32 - 0x2588) as usize;
            let filled_w = w * n / 8;
            fill(&mut m, w, 0, 0, filled_w, h);
        }
        _ => return None,
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_rangos_box_y_block() {
        assert!(is_box_glyph('\u{2500}'));
        assert!(is_box_glyph('\u{250C}'));
        assert!(is_box_glyph('\u{2588}'));
        assert!(is_box_glyph('\u{2591}'));
        assert!(!is_box_glyph('A'));
        assert!(!is_box_glyph(' '));
    }

    #[test]
    fn box_mask_horizontal_pinta_franja_media() {
        let (w, h) = (10usize, 20usize);
        let m = box_mask('\u{2500}', w, h).expect("─ soportado");
        assert_eq!(m.len(), w * h);
        let mid = h / 2;
        assert!(m[mid * w + 5] > 0);
        assert_eq!(m[0], 0);
    }

    #[test]
    fn box_mask_full_block_todo_opaco() {
        let (w, h) = (8usize, 16usize);
        let m = box_mask('\u{2588}', w, h).expect("█ soportado");
        assert!(m.iter().all(|&b| b == 255));
    }

    #[test]
    fn box_mask_lower_half_pinta_mitad_inferior() {
        let (w, h) = (8usize, 16usize);
        let m = box_mask('\u{2584}', w, h).expect("▄ soportado");
        assert_eq!(m[0], 0);
        assert_eq!(m[(h - 1) * w], 255);
    }

    #[test]
    fn box_mask_caracter_no_soportado_es_none() {
        assert!(box_mask('\u{2550}', 8, 16).is_none());
    }
}
