//! Block elements U+2580..U+259F con distribucion eight_range.

use super::stroke::{fill, stroke_light};

/// Distribuye `total` en 8 bandas; banda 0 = inferior, banda 7 = superior.
///
/// Usa redondeo acumulativo (en vez de volcar el resto en las primeras
/// bandas) para que el remanente quede repartido de forma pareja: con el
/// esquema anterior, medio bloque superior (bandas 4..8) y medio bloque
/// inferior (bandas 0..4) podian diferir hasta en 2px para el mismo `total`
/// (p.ej. `▀`/`▄` en una celda de 14px quedaban en 6px y 8px en vez de 7/7).
pub fn eight_range(total: usize) -> [usize; 8] {
    let mut bands = [0usize; 8];
    let mut prev = 0usize;
    for (i, band) in bands.iter_mut().enumerate() {
        let cur = (i + 1) * total / 8;
        *band = cur - prev;
        prev = cur;
    }
    bands
}

/// Rangos Y [y0, y1) por banda (indice 0 = abajo).
fn band_y_ranges(h: usize) -> [(usize, usize); 8] {
    let bands = eight_range(h);
    let mut ranges = [(0usize, 0usize); 8];
    let mut y = h;
    for (i, &bh) in bands.iter().enumerate() {
        y = y.saturating_sub(bh);
        ranges[i] = (y, y + bh);
    }
    ranges
}

fn paint_bands(mask: &mut [u8], w: usize, h: usize, indices: &[usize]) {
    let ranges = band_y_ranges(h);
    for &i in indices {
        let (y0, y1) = ranges[i];
        fill(mask, w, 0, y0, w, y1.min(h));
    }
}

pub fn paint_eight_block_bottom(mask: &mut [u8], w: usize, h: usize, bands_filled: usize) {
    if bands_filled == 0 || h == 0 || w == 0 {
        return;
    }
    let indices: Vec<usize> = (0..bands_filled.min(8)).collect();
    paint_bands(mask, w, h, &indices);
}

pub fn paint_eight_block_top(mask: &mut [u8], w: usize, h: usize, bands_filled: usize) {
    if bands_filled == 0 || h == 0 || w == 0 {
        return;
    }
    let start = 8usize.saturating_sub(bands_filled);
    let indices: Vec<usize> = (start..8).collect();
    paint_bands(mask, w, h, &indices);
}

pub fn paint_left_eighth(mask: &mut [u8], w: usize, h: usize, eighths: usize) {
    if eighths == 0 {
        return;
    }
    let bands = eight_range(w);
    let mut x_end = 0usize;
    for &bw in bands.iter().skip(8usize.saturating_sub(eighths)) {
        x_end += bw;
    }
    fill(mask, w, 0, 0, x_end.min(w), h);
}

pub fn paint_right_eighth(mask: &mut [u8], w: usize, h: usize, eighths: usize) {
    if eighths == 0 {
        return;
    }
    let bands = eight_range(w);
    let mut x_start = w;
    for &bw in bands.iter().take(eighths.min(8)) {
        x_start = x_start.saturating_sub(bw);
    }
    fill(mask, w, x_start, 0, w, h);
}

pub fn render_block(ch: char, w: usize, h: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    let mut m = vec![0u8; w * h];
    match ch {
        '\u{2588}' => fill(&mut m, w, 0, 0, w, h),
        '\u{2580}' => paint_eight_block_top(&mut m, w, h, 4),
        '\u{2584}' => paint_eight_block_bottom(&mut m, w, h, 4),
        '\u{258C}' => paint_left_eighth(&mut m, w, h, 4),
        '\u{2590}' => paint_right_eighth(&mut m, w, h, 4),
        '\u{2594}' => fill(&mut m, w, 0, 0, w, stroke_light(w, h).max(1)),
        '\u{2595}' => paint_right_eighth(&mut m, w, h, 1),
        '\u{2591}' => m.iter_mut().for_each(|p| *p = 64),
        '\u{2592}' => m.iter_mut().for_each(|p| *p = 128),
        '\u{2593}' => m.iter_mut().for_each(|p| *p = 192),
        c @ '\u{2581}'..='\u{2587}' => {
            let n = (c as u32 - 0x2580) as usize;
            paint_eight_block_bottom(&mut m, w, h, n);
        }
        c @ '\u{2589}'..='\u{258F}' => {
            let n = 8 - (c as u32 - 0x2588) as usize;
            paint_left_eighth(&mut m, w, h, n);
        }
        '\u{2596}'..='\u{259F}' => paint_quadrant(&mut m, w, h, ch),
        _ => return None,
    }
    Some(m)
}

fn paint_quadrant(mask: &mut [u8], w: usize, h: usize, ch: char) {
    let midx = w / 2;
    let midy = h / 2;
    let code = ch as u32;
    // Tabla real de cuadrantes Unicode U+2596..U+259F (verificada contra el
    // nombre de cada caracter, p.ej. U+259B = "UPPER LEFT AND UPPER RIGHT
    // AND LOWER LEFT" => tl+tr+bl). El mapeo anterior tenia tl/tr/bl/br
    // transpuestos, por lo que varios caracteres (p.ej. ▙ ▛ ▟) salian con
    // los cuadrantes equivocados.
    let tl = matches!(code, 0x2598..=0x259C);
    let tr = matches!(code, 0x259B..=0x259F);
    let bl = matches!(code, 0x2596 | 0x2599 | 0x259B | 0x259E | 0x259F);
    let br = matches!(code, 0x2597 | 0x2599 | 0x259A | 0x259C | 0x259F);
    if tl {
        fill(mask, w, 0, 0, midx.max(1), midy.max(1));
    }
    if tr {
        fill(mask, w, midx, 0, w, midy.max(1));
    }
    if bl {
        fill(mask, w, 0, midy, midx.max(1), h);
    }
    if br {
        fill(mask, w, midx, midy, w, h);
    }
}

pub fn supports_block(ch: char) -> bool {
    matches!(ch as u32, 0x2580..=0x259F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_range_sums_to_total() {
        for h in 14..=24 {
            let bands = eight_range(h);
            assert_eq!(bands.iter().sum::<usize>(), h);
        }
    }

    #[test]
    fn eighth_blocks_tile_full_cell_without_gaps() {
        let w = 10usize;
        for h in 14..=24 {
            let mut combined = vec![0u8; w * h];
            for n in 1..=8 {
                let ch = char::from_u32(0x2580 + n as u32).unwrap();
                let m = render_block(ch, w, h).expect("eighth");
                for (dst, &src) in combined.iter_mut().zip(m.iter()) {
                    if src > 0 {
                        *dst = 255;
                    }
                }
            }
            assert!(combined.iter().all(|&b| b == 255), "hueco en h={h}");
        }
    }

    #[test]
    fn full_block_solid() {
        let m = render_block('\u{2588}', 8, 16).expect("block");
        assert!(m.iter().all(|&b| b == 255));
    }

    #[test]
    fn upper_and_lower_half_block_are_symmetric() {
        for h in 8..=24 {
            let w = 8usize;
            let top = render_block('\u{2580}', w, h).expect("upper half");
            let bottom = render_block('\u{2584}', w, h).expect("lower half");
            let top_rows = (0..h).filter(|&y| top[y * w] > 0).count();
            let bottom_rows = (0..h).filter(|&y| bottom[y * w] > 0).count();
            assert_eq!(top_rows + bottom_rows, h, "h={h}");
            assert!(
                top_rows.abs_diff(bottom_rows) <= 1,
                "mitades desiguales en h={h}: top={top_rows} bottom={bottom_rows}"
            );
        }
    }

    #[test]
    fn left_and_right_half_blocks_are_symmetric() {
        for w in 8..=16 {
            let h = 12usize;
            let left = render_block('\u{258C}', w, h).expect("left half");
            let right = render_block('\u{2590}', w, h).expect("right half");
            let left_cols = (0..w).filter(|&x| left[x] > 0).count();
            let right_cols = (0..w).filter(|&x| right[x] > 0).count();
            assert_eq!(left_cols + right_cols, w, "w={w}");
            assert!(
                left_cols.abs_diff(right_cols) <= 1,
                "mitades desiguales en w={w}: left={left_cols} right={right_cols}"
            );
            assert!(
                left[0] > 0 && left[w - 1] == 0,
                "U+258C debe ocupar la izquierda w={w}"
            );
            assert!(
                right[0] == 0 && right[w - 1] > 0,
                "U+2590 debe ocupar la derecha w={w}"
            );
            for x in 0..w {
                assert!(
                    !(left[x] > 0 && right[x] > 0),
                    "solape en columna {x} w={w}"
                );
            }
        }
    }

    #[test]
    fn quadrant_chars_match_unicode_definition() {
        // (codepoint, tl, tr, bl, br) segun el nombre Unicode real.
        let cases: &[(u32, bool, bool, bool, bool)] = &[
            (0x2596, false, false, true, false), // QUADRANT LOWER LEFT
            (0x2597, false, false, false, true), // QUADRANT LOWER RIGHT
            (0x2598, true, false, false, false), // QUADRANT UPPER LEFT
            (0x2599, true, false, true, true),   // UPPER LEFT + LOWER LEFT + LOWER RIGHT
            (0x259A, true, false, false, true),  // UPPER LEFT + LOWER RIGHT
            (0x259B, true, true, true, false),   // UPPER LEFT + UPPER RIGHT + LOWER LEFT
            (0x259C, true, true, false, true),   // UPPER LEFT + UPPER RIGHT + LOWER RIGHT
            (0x259D, false, true, false, false), // QUADRANT UPPER RIGHT
            (0x259E, false, true, true, false),  // UPPER RIGHT + LOWER LEFT
            (0x259F, false, true, true, true),   // UPPER RIGHT + LOWER LEFT + LOWER RIGHT
        ];
        let w = 8usize;
        let h = 8usize;
        for &(cp, tl, tr, bl, br) in cases {
            let ch = char::from_u32(cp).unwrap();
            let m = render_block(ch, w, h).expect("quadrant");
            let got_tl = m[0] > 0;
            let got_tr = m[w - 1] > 0;
            let got_bl = m[(h - 1) * w] > 0;
            let got_br = m[(h - 1) * w + (w - 1)] > 0;
            assert_eq!(
                (got_tl, got_tr, got_bl, got_br),
                (tl, tr, bl, br),
                "U+{cp:04X}"
            );
        }
    }
}
