//! Box-drawing U+2500..U+257F.

use super::stroke::{
    paint_arc, paint_cross_diagonal, paint_dashed_horiz, paint_dashed_vert, paint_diagonal,
    paint_segment, stroke_light, Segment, Weight,
};

pub const BOX_DRAWING_START: u32 = 0x2500;
pub const BOX_DRAWING_END: u32 = 0x257F;

pub fn is_box_drawing(ch: char) -> bool {
    matches!(ch as u32, BOX_DRAWING_START..=BOX_DRAWING_END)
}

pub fn supports_box(ch: char) -> bool {
    is_box_drawing(ch)
}

fn classify_segment(ch: char) -> Option<(Weight, Segment)> {
    if !is_box_drawing(ch) {
        return None;
    }
    let o = ch as u32 - BOX_DRAWING_START;
    Some(match ch {
        '\u{2500}' | '\u{2504}' | '\u{2508}' => (Weight::Light, Segment::Horiz),
        '\u{2502}' | '\u{2506}' => (Weight::Light, Segment::Vert),
        '\u{250C}' => (Weight::Light, Segment::CornerTl),
        '\u{2510}' => (Weight::Light, Segment::CornerTr),
        '\u{2514}' => (Weight::Light, Segment::CornerBl),
        '\u{2518}' => (Weight::Light, Segment::CornerBr),
        '\u{251C}' => (Weight::Light, Segment::TeeLeft),
        '\u{2524}' => (Weight::Light, Segment::TeeRight),
        '\u{252C}' => (Weight::Light, Segment::TeeTop),
        '\u{2534}' => (Weight::Light, Segment::TeeBottom),
        '\u{253C}' => (Weight::Light, Segment::Cross),
        '\u{2501}' | '\u{2505}' | '\u{2509}' => (Weight::Heavy, Segment::Horiz),
        '\u{2503}' | '\u{2507}' => (Weight::Heavy, Segment::Vert),
        '\u{250F}' => (Weight::Heavy, Segment::CornerTl),
        '\u{2513}' => (Weight::Heavy, Segment::CornerTr),
        '\u{2517}' => (Weight::Heavy, Segment::CornerBl),
        '\u{251B}' => (Weight::Heavy, Segment::CornerBr),
        '\u{2523}' => (Weight::Heavy, Segment::TeeLeft),
        '\u{252B}' => (Weight::Heavy, Segment::TeeRight),
        '\u{2533}' => (Weight::Heavy, Segment::TeeTop),
        '\u{253B}' => (Weight::Heavy, Segment::TeeBottom),
        '\u{254B}' => (Weight::Heavy, Segment::Cross),
        '\u{2550}' => (Weight::Double, Segment::Horiz),
        '\u{2551}' => (Weight::Double, Segment::Vert),
        '\u{2554}' => (Weight::Double, Segment::CornerTl),
        '\u{2557}' => (Weight::Double, Segment::CornerTr),
        '\u{255A}' => (Weight::Double, Segment::CornerBl),
        '\u{255D}' => (Weight::Double, Segment::CornerBr),
        '\u{2560}' => (Weight::Double, Segment::TeeLeft),
        '\u{2563}' => (Weight::Double, Segment::TeeRight),
        '\u{2566}' => (Weight::Double, Segment::TeeTop),
        '\u{2569}' => (Weight::Double, Segment::TeeBottom),
        '\u{256C}' => (Weight::Double, Segment::Cross),
        _ => fallback_segment(o),
    })
}

fn fallback_segment(o: u32) -> (Weight, Segment) {
    // Variantes Unicode no mapeadas explicitamente: heuristica por offset.
    if o.is_multiple_of(4) {
        (Weight::Light, Segment::Horiz)
    } else if o % 4 == 1 {
        (Weight::Light, Segment::Vert)
    } else if o % 4 == 2 {
        (Weight::Light, Segment::Cross)
    } else {
        (Weight::Light, Segment::Horiz)
    }
}

pub fn render_box(ch: char, w: usize, h: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || !is_box_drawing(ch) {
        return None;
    }
    let stroke = stroke_light(w, h);
    let mut m = vec![0u8; w * h];

    match ch {
        '\u{256D}' => {
            paint_arc(&mut m, w, h, Segment::CornerTl, stroke);
            return Some(m);
        }
        '\u{256E}' => {
            paint_arc(&mut m, w, h, Segment::CornerTr, stroke);
            return Some(m);
        }
        '\u{256F}' => {
            paint_arc(&mut m, w, h, Segment::CornerBr, stroke);
            return Some(m);
        }
        '\u{2570}' => {
            paint_arc(&mut m, w, h, Segment::CornerBl, stroke);
            return Some(m);
        }
        '\u{254C}' | '\u{2504}' | '\u{2505}' => {
            paint_dashed_horiz(&mut m, w, h, stroke);
            return Some(m);
        }
        '\u{254E}' | '\u{2506}' | '\u{2507}' => {
            paint_dashed_vert(&mut m, w, h, stroke);
            return Some(m);
        }
        '\u{2571}' => {
            paint_diagonal(&mut m, w, h, stroke, false);
            return Some(m);
        }
        '\u{2572}' => {
            paint_diagonal(&mut m, w, h, stroke, true);
            return Some(m);
        }
        '\u{2573}' => {
            paint_cross_diagonal(&mut m, w, h, stroke);
            return Some(m);
        }
        _ => {}
    }

    let (weight, seg) = classify_segment(ch)?;
    paint_segment(&mut m, w, h, weight, seg);
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_box_drawing_range_supported() {
        for cp in BOX_DRAWING_START..=BOX_DRAWING_END {
            let ch = char::from_u32(cp).unwrap();
            assert!(supports_box(ch), "U+{cp:X}");
            let m = render_box(ch, 10, 20);
            assert!(m.is_some(), "render U+{cp:X}");
        }
    }

    #[test]
    fn round_corners_non_empty() {
        for ch in ['\u{256D}', '\u{256E}', '\u{256F}', '\u{2570}'] {
            let m = render_box(ch, 12, 24).expect("arc");
            assert!(m.iter().any(|&b| b > 0));
        }
    }

    #[test]
    fn round_corner_is_single_connected_component() {
        let w = 20usize;
        let h = 30usize;
        for ch in ['\u{256D}', '\u{256E}', '\u{256F}', '\u{2570}'] {
            let m = render_box(ch, w, h).expect("arc");
            let total_filled = m.iter().filter(|&&b| b > 0).count();
            assert!(total_filled > 0, "U+{:04X}", ch as u32);

            let start = m.iter().position(|&b| b > 0).unwrap();
            let mut visited = vec![false; m.len()];
            let mut stack = vec![start];
            visited[start] = true;
            let mut reached = 1usize;
            while let Some(idx) = stack.pop() {
                let x = (idx % w) as i32;
                let y = (idx / w) as i32;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx as usize >= w || ny as usize >= h {
                        continue;
                    }
                    let nidx = ny as usize * w + nx as usize;
                    if m[nidx] > 0 && !visited[nidx] {
                        visited[nidx] = true;
                        reached += 1;
                        stack.push(nidx);
                    }
                }
            }
            assert_eq!(
                reached, total_filled,
                "esquina redondeada U+{:04X} quedo fragmentada (arco desconectado de un brazo)",
                ch as u32
            );
        }
    }

    #[test]
    fn vertical_line_full_height() {
        let w = 10usize;
        let h = 20usize;
        let midx = w / 2;
        let m = render_box('\u{2502}', w, h).expect("v");
        assert!(m[midx] > 0);
        assert!(m[(h - 1) * w + midx] > 0);
    }

    #[test]
    fn corner_arms_match_straight_stroke_thickness() {
        let w = 40usize;
        let h = 40usize;
        let s = stroke_light(w, h);
        let m = render_box('\u{250C}', w, h).expect("corner");

        let x = w - 1;
        let horiz_thickness = (0..h).filter(|&y| m[y * w + x] > 0).count();
        assert_eq!(
            horiz_thickness, s,
            "el brazo horizontal no debe engrosarse en el codo"
        );

        let y = h - 1;
        let vert_thickness = (0..w).filter(|&x| m[y * w + x] > 0).count();
        assert_eq!(
            vert_thickness, s,
            "el brazo vertical no debe engrosarse en el codo"
        );
    }

    #[test]
    fn double_corner_has_two_parallel_arms() {
        let w = 40usize;
        let h = 40usize;
        let m = render_box('\u{2554}', w, h).expect("double corner");

        let x = w - 1;
        let mut runs = 0usize;
        let mut in_run = false;
        for y in 0..h {
            let filled = m[y * w + x] > 0;
            if filled && !in_run {
                runs += 1;
            }
            in_run = filled;
        }
        assert_eq!(
            runs, 2,
            "la esquina de linea doble debe tener dos brazos horizontales paralelos, no uno solo"
        );
    }
}
