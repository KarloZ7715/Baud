//! Box-drawing U+2500..U+257F.

use super::stroke::{
    paint_arc, paint_cross_diagonal, paint_dashed_horiz, paint_dashed_vert, paint_diagonal,
    paint_segment, stroke_for, stroke_light, Segment, Weight,
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
    Some(match ch {
        // Light straight
        '\u{2500}' => (Weight::Light, Segment::Horiz),
        '\u{2502}' => (Weight::Light, Segment::Vert),
        // Heavy straight
        '\u{2501}' => (Weight::Heavy, Segment::Horiz),
        '\u{2503}' => (Weight::Heavy, Segment::Vert),
        // Light corners / tees / cross
        '\u{250C}' | '\u{250D}' | '\u{250E}' => (Weight::Light, Segment::CornerTl),
        '\u{2510}' | '\u{2511}' | '\u{2512}' => (Weight::Light, Segment::CornerTr),
        '\u{2514}' | '\u{2515}' | '\u{2516}' => (Weight::Light, Segment::CornerBl),
        '\u{2518}' | '\u{2519}' | '\u{251A}' => (Weight::Light, Segment::CornerBr),
        '\u{251C}' | '\u{251D}' | '\u{251E}' | '\u{251F}' | '\u{2520}' | '\u{2521}'
        | '\u{2522}' => (Weight::Light, Segment::TeeLeft),
        '\u{2524}' | '\u{2525}' | '\u{2526}' | '\u{2527}' | '\u{2528}' | '\u{2529}'
        | '\u{252A}' => (Weight::Light, Segment::TeeRight),
        '\u{252C}' | '\u{252D}' | '\u{252E}' | '\u{252F}' | '\u{2530}' | '\u{2531}'
        | '\u{2532}' => (Weight::Light, Segment::TeeTop),
        '\u{2534}' | '\u{2535}' | '\u{2536}' | '\u{2537}' | '\u{2538}' | '\u{2539}'
        | '\u{253A}' => (Weight::Light, Segment::TeeBottom),
        '\u{253C}' | '\u{253D}' | '\u{253E}' | '\u{253F}' | '\u{2540}' | '\u{2541}'
        | '\u{2542}' | '\u{2543}' | '\u{2544}' | '\u{2545}' | '\u{2546}' | '\u{2547}'
        | '\u{2548}' | '\u{2549}' | '\u{254A}' => (Weight::Light, Segment::Cross),
        // Heavy corners / tees / cross
        '\u{250F}' => (Weight::Heavy, Segment::CornerTl),
        '\u{2513}' => (Weight::Heavy, Segment::CornerTr),
        '\u{2517}' => (Weight::Heavy, Segment::CornerBl),
        '\u{251B}' => (Weight::Heavy, Segment::CornerBr),
        '\u{2523}' => (Weight::Heavy, Segment::TeeLeft),
        '\u{252B}' => (Weight::Heavy, Segment::TeeRight),
        '\u{2533}' => (Weight::Heavy, Segment::TeeTop),
        '\u{253B}' => (Weight::Heavy, Segment::TeeBottom),
        '\u{254B}' => (Weight::Heavy, Segment::Cross),
        // Double
        '\u{2550}' => (Weight::Double, Segment::Horiz),
        '\u{2551}' => (Weight::Double, Segment::Vert),
        '\u{2552}' | '\u{2553}' | '\u{2554}' => (Weight::Double, Segment::CornerTl),
        '\u{2555}' | '\u{2556}' | '\u{2557}' => (Weight::Double, Segment::CornerTr),
        '\u{2558}' | '\u{2559}' | '\u{255A}' => (Weight::Double, Segment::CornerBl),
        '\u{255B}' | '\u{255C}' | '\u{255D}' => (Weight::Double, Segment::CornerBr),
        '\u{255E}' | '\u{255F}' | '\u{2560}' => (Weight::Double, Segment::TeeLeft),
        '\u{2561}' | '\u{2562}' | '\u{2563}' => (Weight::Double, Segment::TeeRight),
        '\u{2564}' | '\u{2565}' | '\u{2566}' => (Weight::Double, Segment::TeeTop),
        '\u{2567}' | '\u{2568}' | '\u{2569}' => (Weight::Double, Segment::TeeBottom),
        '\u{256A}' | '\u{256B}' | '\u{256C}' => (Weight::Double, Segment::Cross),
        // Half lines
        '\u{2574}' => (Weight::Light, Segment::HorizLeft),
        '\u{2575}' => (Weight::Light, Segment::VertUp),
        '\u{2576}' => (Weight::Light, Segment::HorizRight),
        '\u{2577}' => (Weight::Light, Segment::VertDown),
        '\u{2578}' => (Weight::Heavy, Segment::HorizLeft),
        '\u{2579}' => (Weight::Heavy, Segment::VertUp),
        '\u{257A}' => (Weight::Heavy, Segment::HorizRight),
        '\u{257B}' => (Weight::Heavy, Segment::VertDown),
        // Mixed-weight full lines (topologia: linea completa)
        '\u{257C}' | '\u{257E}' => (Weight::Light, Segment::Horiz),
        '\u{257D}' | '\u{257F}' => (Weight::Light, Segment::Vert),
        // Arcs / dashed / diagonals se resuelven en render_box antes de classify.
        _ => (Weight::Light, Segment::Horiz),
    })
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
        '\u{254C}' | '\u{2504}' | '\u{2508}' => {
            paint_dashed_horiz(&mut m, w, h, stroke);
            return Some(m);
        }
        '\u{2505}' | '\u{2509}' | '\u{254D}' => {
            paint_dashed_horiz(&mut m, w, h, stroke_for(Weight::Heavy, w, h));
            return Some(m);
        }
        '\u{254E}' | '\u{2506}' | '\u{250A}' => {
            paint_dashed_vert(&mut m, w, h, stroke);
            return Some(m);
        }
        '\u{2507}' | '\u{250B}' | '\u{254F}' => {
            paint_dashed_vert(&mut m, w, h, stroke_for(Weight::Heavy, w, h));
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

    #[test]
    fn dashed_horiz_has_gaps_not_full_solid() {
        for ch in [
            '\u{2504}', '\u{254C}', '\u{2508}', '\u{2505}', '\u{2509}', '\u{254D}',
        ] {
            let w = 24usize;
            let h = 20usize;
            let m = render_box(ch, w, h).expect("dashed");
            let midy = h / 2;
            let filled: Vec<bool> = (0..w).map(|x| m[midy * w + x] > 0).collect();
            let gaps = filled.iter().filter(|&&f| !f).count();
            let solids = filled.iter().filter(|&&f| f).count();
            assert!(
                solids > 0 && gaps > 0,
                "U+{:04X} debe ser discontinua (solidos={solids} huecos={gaps})",
                ch as u32
            );
        }
    }

    #[test]
    fn dashed_vert_has_gaps_not_full_solid() {
        for ch in [
            '\u{2506}', '\u{254E}', '\u{250A}', '\u{2507}', '\u{250B}', '\u{254F}',
        ] {
            let w = 12usize;
            let h = 24usize;
            let m = render_box(ch, w, h).expect("dashed");
            let midx = w / 2;
            let filled: Vec<bool> = (0..h).map(|y| m[y * w + midx] > 0).collect();
            let gaps = filled.iter().filter(|&&f| !f).count();
            let solids = filled.iter().filter(|&&f| f).count();
            assert!(
                solids > 0 && gaps > 0,
                "U+{:04X} debe ser discontinua (solidos={solids} huecos={gaps})",
                ch as u32
            );
        }
    }

    #[test]
    fn half_lines_occupy_only_their_half() {
        let w = 20usize;
        let h = 20usize;
        let midx = w / 2;
        let midy = h / 2;

        for ch in ['\u{2574}', '\u{2578}'] {
            let left = render_box(ch, w, h).expect("left");
            assert!(left[midy * w] > 0, "LEFT debe pintar borde izquierdo");
            for x in (3 * w / 4)..w {
                assert_eq!(left[midy * w + x], 0, "U+{:04X} x={x}", ch as u32);
            }
        }

        for ch in ['\u{2576}', '\u{257A}'] {
            let right = render_box(ch, w, h).expect("right");
            assert!(
                right[midy * w + (w - 1)] > 0,
                "RIGHT debe pintar borde derecho"
            );
            for x in 0..(w / 4) {
                assert_eq!(right[midy * w + x], 0, "U+{:04X} x={x}", ch as u32);
            }
        }

        for ch in ['\u{2575}', '\u{2579}'] {
            let up = render_box(ch, w, h).expect("up");
            assert!(up[midx] > 0, "UP debe pintar borde superior");
            for y in (3 * h / 4)..h {
                assert_eq!(up[y * w + midx], 0, "U+{:04X} y={y}", ch as u32);
            }
        }

        for ch in ['\u{2577}', '\u{257B}'] {
            let down = render_box(ch, w, h).expect("down");
            assert!(
                down[(h - 1) * w + midx] > 0,
                "DOWN debe pintar borde inferior"
            );
            for y in 0..(h / 4) {
                assert_eq!(down[y * w + midx], 0, "U+{:04X} y={y}", ch as u32);
            }
        }
    }

    #[test]
    fn corner_meets_adjacent_straight_lines() {
        let w = 16usize;
        let h = 16usize;
        let corner = render_box('\u{250C}', w, h).expect("corner");
        let horiz = render_box('\u{2500}', w, h).expect("horiz");
        let vert = render_box('\u{2502}', w, h).expect("vert");
        let midy = h / 2;
        let midx = w / 2;
        // El brazo derecho de ┌ y la ─ vecina deben compartir la franja media en el borde.
        assert!(corner[midy * w + (w - 1)] > 0 && horiz[midy * w] > 0);
        // El brazo inferior de ┌ y la │ vecina deben compartir la columna media en el borde.
        assert!(corner[(h - 1) * w + midx] > 0 && vert[midx] > 0);
    }

    #[test]
    fn supports_rejects_outside_box_block_ranges() {
        assert!(!supports_box('A'));
        assert!(!supports_box('\u{24FF}'));
        assert!(!supports_box('\u{2580}')); // block elements, no box-drawing
    }
}
