//! Separadores Powerline U+E0B0..=U+E0B3 anclados a la celda.

use super::stroke::{fill, stroke_light};

pub fn is_powerline_sep(ch: char) -> bool {
    matches!(
        ch as u32,
        super::POWERLINE_SEP_START..=super::POWERLINE_SEP_END
    )
}

pub fn supports_powerline(ch: char) -> bool {
    is_powerline_sep(ch)
}

pub fn render_powerline(ch: char, w: usize, h: usize) -> Option<Vec<u8>> {
    if !supports_powerline(ch) || w == 0 || h == 0 {
        return None;
    }
    let mut mask = vec![0u8; w * h];
    let solid = matches!(ch, '\u{E0B0}' | '\u{E0B2}');
    let rtl = matches!(ch, '\u{E0B2}' | '\u{E0B3}');
    paint_chevron_ltr(&mut mask, w, h, solid);
    if rtl {
        flip_horizontal(&mut mask, w, h);
    }
    Some(mask)
}

/// Chevron con base en el borde izquierdo y punta hacia la derecha (pendiente 1).
fn paint_chevron_ltr(mask: &mut [u8], w: usize, h: usize, solid: bool) {
    if h < 2 {
        fill(mask, w, 0, 0, w, h);
        return;
    }
    let x_tip = (h - 1) / 2;
    let stroke = stroke_light(w, h).max(1);

    for x in 0..=x_tip {
        if x >= w {
            break;
        }
        let y_top = x;
        let y_bot = (h - 1).saturating_sub(x);
        if y_top > y_bot {
            break;
        }
        let x_end = (x + 1).min(w);
        if solid {
            fill(mask, w, 0, y_top, x_end, y_top + 1);
            if y_bot != y_top {
                fill(mask, w, 0, y_bot, x_end, y_bot + 1);
            }
        } else {
            let inner = x.saturating_sub(stroke.saturating_sub(1));
            if x + 1 >= w {
                // Punta cortada: cierre vertical entre diagonales.
                fill(mask, w, x, y_top, x + 1, y_bot + 1);
                break;
            }
            let y_top_inner = (y_top + stroke).min(y_bot);
            fill(
                mask,
                w,
                inner,
                y_top,
                x_end,
                y_top_inner.min(y_top + stroke),
            );
            if y_bot > y_top {
                let y_bot_inner = y_bot.saturating_sub(stroke - 1);
                fill(mask, w, inner, y_bot_inner.max(y_top), x_end, y_bot + 1);
            }
        }
    }
}

fn flip_horizontal(mask: &mut [u8], w: usize, h: usize) {
    for y in 0..h {
        let row = y * w;
        for x in 0..(w / 2) {
            mask.swap(row + x, row + (w - 1 - x));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubre_solo_separadores_no_iconos() {
        assert!(supports_powerline('\u{E0B0}'));
        assert!(supports_powerline('\u{E0B3}'));
        assert!(!supports_powerline('\u{E0A0}'));
        assert!(!supports_powerline('\u{2500}'));
    }

    #[test]
    fn triangulo_ltr_lleno_altura_completa_con_punta() {
        let w = 12usize;
        let h = 20usize;
        let mask = render_powerline('\u{E0B0}', w, h).expect("mask");
        assert_eq!(mask.len(), w * h);
        assert!(mask.iter().any(|&p| p > 0), "mascara no vacia");
        // Filas extremas: al menos un pixel cerca del borde izquierdo.
        assert!(mask[0] > 0 || mask[1] > 0);
        assert!(mask[(h - 1) * w] > 0 || mask[(h - 1) * w + 1] > 0);
        // Fila media: el relleno llega mas lejos a la derecha que en los extremos.
        let mid = h / 2;
        let mid_extent = (0..w).rev().find(|&x| mask[mid * w + x] > 0).unwrap_or(0);
        let top_extent = (0..w).rev().find(|&x| mask[x] > 0).unwrap_or(0);
        assert!(mid_extent > top_extent);
    }

    #[test]
    fn triangulo_rtl_es_espejo_del_ltr() {
        let w = 10usize;
        let h = 16usize;
        let ltr = render_powerline('\u{E0B0}', w, h).expect("ltr");
        let rtl = render_powerline('\u{E0B2}', w, h).expect("rtl");
        for y in 0..h {
            for x in 0..w {
                assert_eq!(
                    ltr[y * w + x],
                    rtl[y * w + (w - 1 - x)],
                    "espejo en ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn flecha_hueca_no_rellena_el_centro() {
        let w = 14usize;
        let h = 20usize;
        let solid = render_powerline('\u{E0B0}', w, h).expect("solid");
        let hollow = render_powerline('\u{E0B1}', w, h).expect("hollow");
        let mid = h / 2;
        let hollow_fill: usize = hollow.iter().map(|&p| (p > 0) as usize).sum();
        let solid_fill: usize = solid.iter().map(|&p| (p > 0) as usize).sum();
        assert!(hollow_fill < solid_fill);
        assert!(hollow_fill > 0);
        // Interior cerca de la base: solido relleno, hueco no.
        assert!(solid[mid * w + 1] > 0);
        assert_eq!(hollow[mid * w + 1], 0);
    }
}
