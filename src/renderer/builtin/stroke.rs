//! Primitivas de trazo para box-drawing (hline, vline, junctions).

/// Grosor de linea light (~1/8 de la celda, minimo 1px).
pub fn stroke_light(w: usize, h: usize) -> usize {
    (w.min(h) / 8).max(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Light,
    Heavy,
    Double,
}

pub fn stroke_for(weight: Weight, w: usize, h: usize) -> usize {
    let light = stroke_light(w, h);
    match weight {
        Weight::Light => light,
        Weight::Heavy => (light * 2).clamp(2, w.min(h)),
        Weight::Double => light,
    }
}

/// Rellena [x0,x1) x [y0,y1) con 255.
pub fn fill(mask: &mut [u8], w: usize, x0: usize, y0: usize, x1: usize, y1: usize) {
    let max_y = mask.len() / w;
    for y in y0..y1.min(max_y) {
        for x in x0..x1.min(w) {
            mask[y * w + x] = 255;
        }
    }
}

pub fn horiz_band(mask: &mut [u8], w: usize, h: usize, y_center: usize, stroke: usize) {
    let y0 = y_center.saturating_sub(stroke / 2);
    let y1 = (y_center + stroke - stroke / 2).min(h);
    fill(mask, w, 0, y0, w, y1);
}

pub fn vert_band(mask: &mut [u8], w: usize, h: usize, x_center: usize, stroke: usize) {
    let x0 = x_center.saturating_sub(stroke / 2);
    let x1 = (x_center + stroke - stroke / 2).min(w);
    fill(mask, w, x0, 0, x1, h);
}

pub fn paint_horiz(mask: &mut [u8], w: usize, h: usize, weight: Weight) {
    match weight {
        Weight::Light | Weight::Heavy => {
            horiz_band(mask, w, h, h / 2, stroke_for(weight, w, h));
        }
        Weight::Double => {
            let s = stroke_for(weight, w, h);
            horiz_band(mask, w, h, h / 3, s);
            horiz_band(mask, w, h, (2 * h) / 3, s);
        }
    }
}

pub fn paint_vert(mask: &mut [u8], w: usize, h: usize, weight: Weight) {
    match weight {
        Weight::Light | Weight::Heavy => {
            vert_band(mask, w, h, w / 2, stroke_for(weight, w, h));
        }
        Weight::Double => {
            let s = stroke_for(weight, w, h);
            let x1 = w / 3;
            let x2 = (2 * w) / 3;
            vert_band(mask, w, h, x1, s);
            vert_band(mask, w, h, x2.max(x1 + 1).min(w.saturating_sub(1)), s);
        }
    }
}

fn paint_half_horiz(
    mask: &mut [u8],
    w: usize,
    h: usize,
    weight: Weight,
    from_left: bool,
    extend_by: usize,
) {
    let midx = w / 2;
    let s = stroke_for(weight, w, h);
    let midy = h / 2;
    // `extend_by` solo alarga el brazo hacia el centro para que el codo quede
    // cubierto; el grosor transversal (eje Y) es siempre `s`, igual que en un
    // tramo recto.
    let y0 = midy.saturating_sub(s / 2);
    let y1 = (midy + s - s / 2).min(h);
    if from_left {
        fill(mask, w, 0, y0, (midx + extend_by).min(w), y1);
    } else {
        fill(mask, w, midx.saturating_sub(extend_by), y0, w, y1);
    }
}

fn paint_half_vert(
    mask: &mut [u8],
    w: usize,
    h: usize,
    weight: Weight,
    from_top: bool,
    extend_by: usize,
) {
    let midx = w / 2;
    let s = stroke_for(weight, w, h);
    let midy = h / 2;
    // Mismo criterio que `paint_half_horiz`: `extend_by` solo alarga el brazo
    // hacia el centro; el grosor transversal (eje X) es siempre `s`.
    let x0 = midx.saturating_sub(s / 2);
    let x1 = (midx + s - s / 2).min(w);
    if from_top {
        fill(mask, w, x0, 0, x1, (midy + extend_by).min(h));
    } else {
        fill(mask, w, x0, midy.saturating_sub(extend_by), x1, h);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Horiz,
    Vert,
    HorizLeft,
    HorizRight,
    VertUp,
    VertDown,
    CornerTl,
    CornerTr,
    CornerBl,
    CornerBr,
    TeeLeft,
    TeeRight,
    TeeTop,
    TeeBottom,
    Cross,
}

fn horiz_band_range(
    mask: &mut [u8],
    w: usize,
    h: usize,
    y_center: usize,
    stroke: usize,
    x0: usize,
    x1: usize,
) {
    let y0 = y_center.saturating_sub(stroke / 2);
    let y1 = (y_center + stroke - stroke / 2).min(h);
    fill(mask, w, x0, y0, x1, y1);
}

fn vert_band_range(
    mask: &mut [u8],
    w: usize,
    x_center: usize,
    stroke: usize,
    y0: usize,
    y1: usize,
) {
    let x0 = x_center.saturating_sub(stroke / 2);
    let x1 = (x_center + stroke - stroke / 2).min(w);
    fill(mask, w, x0, y0, x1, y1);
}

/// Esquina de linea doble: dos "L" anidadas (marco exterior + interior),
/// en vez de colapsar a un solo trazo como los pesos Light/Heavy.
fn paint_double_corner(mask: &mut [u8], w: usize, h: usize, corner: Segment) {
    let s = stroke_light(w, h);
    let hy = [h / 3, (2 * h) / 3];
    let vx = [w / 3, (2 * w) / 3];
    match corner {
        Segment::CornerTl => {
            horiz_band_range(mask, w, h, hy[0], s, vx[0], w);
            horiz_band_range(mask, w, h, hy[1], s, vx[1], w);
            vert_band_range(mask, w, vx[0], s, hy[0], h);
            vert_band_range(mask, w, vx[1], s, hy[1], h);
        }
        Segment::CornerTr => {
            horiz_band_range(mask, w, h, hy[0], s, 0, (vx[1] + s).min(w));
            horiz_band_range(mask, w, h, hy[1], s, 0, (vx[0] + s).min(w));
            vert_band_range(mask, w, vx[0], s, hy[1], h);
            vert_band_range(mask, w, vx[1], s, hy[0], h);
        }
        Segment::CornerBl => {
            horiz_band_range(mask, w, h, hy[0], s, vx[1], w);
            horiz_band_range(mask, w, h, hy[1], s, vx[0], w);
            vert_band_range(mask, w, vx[0], s, 0, (hy[1] + s).min(h));
            vert_band_range(mask, w, vx[1], s, 0, (hy[0] + s).min(h));
        }
        Segment::CornerBr => {
            horiz_band_range(mask, w, h, hy[0], s, 0, (vx[0] + s).min(w));
            horiz_band_range(mask, w, h, hy[1], s, 0, (vx[1] + s).min(w));
            vert_band_range(mask, w, vx[0], s, 0, (hy[0] + s).min(h));
            vert_band_range(mask, w, vx[1], s, 0, (hy[1] + s).min(h));
        }
        _ => {}
    }
}

pub fn paint_segment(mask: &mut [u8], w: usize, h: usize, weight: Weight, seg: Segment) {
    let is_double_corner = weight == Weight::Double
        && matches!(
            seg,
            Segment::CornerTl | Segment::CornerTr | Segment::CornerBl | Segment::CornerBr
        );
    if is_double_corner {
        return paint_double_corner(mask, w, h, seg);
    }

    let extend = stroke_for(weight, w, h) / 2;
    match seg {
        Segment::Horiz => paint_horiz(mask, w, h, weight),
        Segment::Vert => paint_vert(mask, w, h, weight),
        Segment::HorizLeft => paint_half_horiz(mask, w, h, weight, true, extend),
        Segment::HorizRight => paint_half_horiz(mask, w, h, weight, false, extend),
        Segment::VertUp => paint_half_vert(mask, w, h, weight, true, extend),
        Segment::VertDown => paint_half_vert(mask, w, h, weight, false, extend),
        Segment::CornerTl => {
            paint_half_horiz(mask, w, h, weight, false, extend);
            paint_half_vert(mask, w, h, weight, false, extend);
        }
        Segment::CornerTr => {
            paint_half_horiz(mask, w, h, weight, true, extend);
            paint_half_vert(mask, w, h, weight, false, extend);
        }
        Segment::CornerBl => {
            paint_half_horiz(mask, w, h, weight, false, extend);
            paint_half_vert(mask, w, h, weight, true, extend);
        }
        Segment::CornerBr => {
            paint_half_horiz(mask, w, h, weight, true, extend);
            paint_half_vert(mask, w, h, weight, true, extend);
        }
        // Tees/cruz de linea doble usan un solo trazo (sin anidar las dos
        // lineas como en las esquinas).
        Segment::TeeLeft => {
            paint_vert(mask, w, h, weight);
            paint_half_horiz(mask, w, h, weight, false, extend);
        }
        Segment::TeeRight => {
            paint_vert(mask, w, h, weight);
            paint_half_horiz(mask, w, h, weight, true, extend);
        }
        Segment::TeeTop => {
            paint_horiz(mask, w, h, weight);
            paint_half_vert(mask, w, h, weight, false, extend);
        }
        Segment::TeeBottom => {
            paint_horiz(mask, w, h, weight);
            paint_half_vert(mask, w, h, weight, true, extend);
        }
        Segment::Cross => {
            paint_horiz(mask, w, h, weight);
            paint_vert(mask, w, h, weight);
        }
    }
}

/// Cuarto de circulo en esquina (arcs light round).
///
/// El punto que se redondea es donde los dos brazos rectos se unirian con
/// angulo recto (el centro de la celda, `(midx, midy)`), NO la esquina
/// fisica de la celda: `╭`/`╮`/`╯`/`╰` deben alinear su brazo horizontal
/// con la fila de un `─` vecino y su brazo vertical con la columna de un `│`
/// vecino, y ambos pasan por el centro de celda. Version anterior centraba
/// el circulo en la esquina fisica (0,0) con radio = ancho completo de la
/// celda, lo que dejaba un hueco entre el arco y los brazos (esquinas
/// "rotas"/desconectadas).
///
/// Tecnica: se recortan ambos brazos `radius` px antes del punto de union,
/// y un arco de ese mismo radio (centrado hacia el lado relleno) cierra el
/// hueco quedando tangente a los dos brazos.
pub fn paint_arc(mask: &mut [u8], w: usize, h: usize, corner: Segment, stroke: usize) {
    let midx = w / 2;
    let midy = h / 2;
    let (from_left, from_top) = match corner {
        Segment::CornerTl => (false, false),
        Segment::CornerTr => (true, false),
        Segment::CornerBl => (false, true),
        Segment::CornerBr => (true, true),
        _ => return,
    };
    let s = stroke.max(1);
    let cap = midx.min(midy).saturating_sub(1);
    if cap == 0 {
        // Celda demasiado chica para curvar sin perder el brazo: esquina recta.
        paint_half_horiz(mask, w, h, Weight::Light, from_left, s / 2);
        paint_half_vert(mask, w, h, Weight::Light, from_top, s / 2);
        return;
    }
    let radius = (s * 3).max(2).min(cap);

    let y0 = midy.saturating_sub(s / 2);
    let y1 = (midy + s - s / 2).min(h);
    if from_left {
        fill(mask, w, 0, y0, midx.saturating_sub(radius), y1);
    } else {
        fill(mask, w, (midx + radius).min(w), y0, w, y1);
    }

    let x0 = midx.saturating_sub(s / 2);
    let x1 = (midx + s - s / 2).min(w);
    if from_top {
        fill(mask, w, x0, 0, x1, midy.saturating_sub(radius));
    } else {
        fill(mask, w, x0, (midy + radius).min(h), x1, h);
    }

    // Centro del arco: desplazado `radius` hacia el lado relleno (donde
    // estan los brazos), para quedar tangente a ambos en el punto de corte.
    let hx: i32 = if from_left { -1 } else { 1 };
    let hy: i32 = if from_top { -1 } else { 1 };
    let cx = midx as i32 + hx * radius as i32;
    let cy = midy as i32 + hy * radius as i32;
    let r = radius as i32;
    let r2 = r * r;
    let inner = (r - s as i32).max(0).pow(2);
    for dy in -r..=r {
        if dy * hy > 0 {
            continue; // solo el cuadrante hacia la muesca (lado vacio).
        }
        for dx in -r..=r {
            if dx * hx > 0 {
                continue;
            }
            let dist = dx * dx + dy * dy;
            if dist > r2 || dist < inner {
                continue;
            }
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                mask[py as usize * w + px as usize] = 255;
            }
        }
    }
}

pub fn paint_dashed_horiz(mask: &mut [u8], w: usize, h: usize, stroke: usize) {
    let midy = h / 2;
    let dash = (w / 6).max(2);
    let gap = (dash / 2).max(1);
    let y0 = midy.saturating_sub(stroke / 2);
    let y1 = (midy + stroke - stroke / 2).min(h);
    let mut x = 0;
    while x < w {
        let x1 = (x + dash).min(w);
        fill(mask, w, x, y0, x1, y1);
        x += dash + gap;
    }
}

pub fn paint_dashed_vert(mask: &mut [u8], w: usize, h: usize, stroke: usize) {
    let midx = w / 2;
    let dash = (h / 6).max(2);
    let gap = dash / 2;
    let mut y = 0;
    while y < h {
        let y1 = (y + dash).min(h);
        for yy in y..y1 {
            for xx in midx.saturating_sub(stroke / 2)..(midx + stroke - stroke / 2).min(w) {
                mask[yy * w + xx] = 255;
            }
        }
        y += dash + gap;
    }
}

pub fn paint_diagonal(
    mask: &mut [u8],
    w: usize,
    h: usize,
    stroke: usize,
    upper_left_to_lower_right: bool,
) {
    let steps = w.max(h) * 2;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let (x, y) = if upper_left_to_lower_right {
            ((t * w as f32) as usize, (t * h as f32) as usize)
        } else {
            (
                (t * w as f32) as usize,
                (h - 1).saturating_sub((t * h as f32) as usize),
            )
        };
        for ds in 0..stroke {
            let xx = x.saturating_add(ds).min(w.saturating_sub(1));
            let yy = y.min(h.saturating_sub(1));
            mask[yy * w + xx] = 255;
            if y > 0 {
                mask[(y - 1).min(h - 1) * w + xx] = 255;
            }
        }
    }
}

pub fn paint_cross_diagonal(mask: &mut [u8], w: usize, h: usize, stroke: usize) {
    paint_diagonal(mask, w, h, stroke, true);
    paint_diagonal(mask, w, h, stroke, false);
}
