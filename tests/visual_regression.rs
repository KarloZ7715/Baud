//! Tests de conectividad de mascaras builtin (regresion visual en CI).

use baud::renderer::{box_mask, is_box_mask_supported, render_builtin_uncached};

#[test]
fn horizontal_line_full_width_coverage() {
    let w = 10usize;
    let h = 20usize;
    let m = render_builtin_uncached('\u{2500}', w, h).expect("h");
    let midy = h / 2;
    let filled = (0..w).filter(|&x| m[midy * w + x] > 0).count();
    assert_eq!(filled, w, "hline debe cubrir todo el ancho de celda");
}

#[test]
fn horizontal_and_vertical_same_stroke_weight() {
    let w = 10usize;
    let h = 20usize;
    let hline = render_builtin_uncached('\u{2500}', w, h).expect("h");
    let vline = render_builtin_uncached('\u{2502}', w, h).expect("v");
    let h_rows = hline
        .chunks(w)
        .filter(|row| row.iter().any(|&p| p > 0))
        .count();
    let v_cols = (0..w)
        .filter(|&x| (0..h).any(|y| vline[y * w + x] > 0))
        .count();
    assert!(h_rows >= 1, "hline al menos 1px");
    assert!(v_cols >= 1, "vline al menos 1px");
    assert_eq!(h_rows, v_cols, "h y v mismo grosor");
}

#[test]
fn vertical_line_three_row_continuity() {
    let w = 10u32;
    let h = 20u32;
    let midx = (w / 2) as usize;
    let m = render_builtin_uncached('\u{2502}', w as usize, h as usize).expect("v");
    assert!(m[midx] > 0, "top");
    assert!(m[(h as usize - 1) * w as usize + midx] > 0, "bottom");
}

#[test]
fn diagonal_chars_supported() {
    for ch in ['\u{2571}', '\u{2572}', '\u{2573}'] {
        assert!(is_box_mask_supported(ch));
        let m = render_builtin_uncached(ch, 12, 24).expect("diag");
        assert!(m.iter().any(|&b| b > 0));
    }
}

#[test]
fn builtin_box_drawing_full_range_non_empty() {
    for cp in 0x2500..=0x257F {
        let ch = char::from_u32(cp).unwrap();
        let m = render_builtin_uncached(ch, 10, 20).expect("box");
        assert!(m.iter().any(|&b| b > 0), "U+{cp:X}");
    }
}

#[test]
fn builtin_block_range_non_empty() {
    for cp in 0x2580..=0x259F {
        let ch = char::from_u32(cp).unwrap();
        let m = box_mask(ch, 10, 20).expect("block");
        assert!(m.iter().any(|&b| b > 0), "U+{cp:X}");
    }
}

#[test]
fn dashed_box_glyphs_have_gaps() {
    for ch in ['\u{2504}', '\u{2506}', '\u{254C}', '\u{254E}'] {
        let w = 24usize;
        let h = 24usize;
        let m = render_builtin_uncached(ch, w, h).expect("dashed");
        let mid_row_gaps = (0..w).filter(|&x| m[(h / 2) * w + x] == 0).count();
        let mid_col_gaps = (0..h).filter(|&y| m[y * w + (w / 2)] == 0).count();
        assert!(
            mid_row_gaps > 0 || mid_col_gaps > 0,
            "U+{:04X} no debe colapsar a trazo solido continuo",
            ch as u32
        );
    }
}
