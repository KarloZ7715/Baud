//! Medicion de celda de grid (ancho, alto, baseline).

use glyphon::cosmic_text::{FontSystem, Hinting, Metrics, Shaping};
use glyphon::fontdb::{Query, Stretch, Style, Weight};

use crate::config::GlyphOffset;

use super::geometry::CellGeometry;
use super::resolve_family;

/// Dimensiones y offsets de una celda de grid en pixeles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    /// Geometria entera de celda (fuente de verdad para builtins).
    pub geometry: CellGeometry,
    pub cell_w: f32,
    pub cell_h: f32,
    pub font_size: f32,
    /// Y de la baseline respecto al borde superior de la celda.
    pub baseline_y: f32,
    /// Posicion del subrayado respecto a la baseline (px).
    pub underline_position: f32,
    /// Grosor del subrayado (px).
    pub underline_thickness: f32,
    /// Factor DPI de la ventana (1.0 = 100 %). Las constantes de decoracion
    /// se multiplican por este valor.
    pub scale_factor: f32,
    /// Y del tachado respecto al borde superior de la celda (px).
    pub strike_y: f32,
    /// Grosor del tachado (px).
    pub strike_thickness: f32,
    pub glyph_offset_x: f32,
    pub glyph_offset_y: f32,
    pub padding_x: f32,
    pub padding_y: f32,
}

impl CellMetrics {
    /// Mide `cell_w`, `cell_h` y `baseline_y` para la familia y tamano dados.
    ///
    /// `scale_factor` es el DPI de la ventana; las metricas de fuente ya
    /// llegan en pixeles fisicos cuando el llamante pasa `font_size * scale`.
    pub fn measure(
        font_system: &mut FontSystem,
        family: &str,
        font_size: f32,
        line_height: f32,
        glyph_offset: GlyphOffset,
        scale_factor: f32,
    ) -> Self {
        let cell_h_f = font_size * line_height;
        let metrics = Metrics::new(font_size, cell_h_f);
        let cell_w_f = measure_cell_width(font_system, metrics, family, font_size);
        let geometry = CellGeometry::new(cell_w_f, cell_h_f);
        let cell_w = geometry.cell_w as f32;
        let cell_h = geometry.cell_h as f32;
        let baseline_y = measure_baseline_y(font_system, metrics, family, cell_w, cell_h);
        let deco = measure_decoration_metrics(font_system, family, font_size, baseline_y, cell_h);
        let scale_factor = scale_factor.max(0.01);

        Self {
            geometry,
            cell_w,
            cell_h,
            font_size,
            baseline_y,
            underline_position: deco.underline_position,
            underline_thickness: deco.underline_thickness,
            scale_factor,
            strike_y: deco.strike_y,
            strike_thickness: deco.strike_thickness,
            glyph_offset_x: glyph_offset.x,
            glyph_offset_y: glyph_offset.y,
            padding_x: 0.0,
            padding_y: 0.0,
        }
    }
}

struct DecorationMetrics {
    underline_position: f32,
    underline_thickness: f32,
    strike_y: f32,
    strike_thickness: f32,
}

/// Lee subrayado/tachado de las tablas `post`/`OS/2` via la cara cargada.
///
/// Si la fuente no expone esas metricas, cae a la aproximacion
/// `font_size * 0.1` / `0.05` (subrayado) y `baseline - font_size*0.25` (tachado).
fn measure_decoration_metrics(
    font_system: &mut FontSystem,
    family: &str,
    font_size: f32,
    baseline_y: f32,
    cell_h: f32,
) -> DecorationMetrics {
    let approx_ul_pos = (font_size * 0.1).max(1.0);
    let approx_ul_thick = (font_size * 0.05).max(1.0);
    let approx_strike_thick = approx_ul_thick;
    let approx_strike_y = (baseline_y - font_size * 0.25).clamp(0.0, (cell_h - 1.0).max(0.0));

    let query = Query {
        families: &[resolve_family(family)],
        weight: Weight::NORMAL,
        stretch: Stretch::Normal,
        style: Style::Normal,
    };
    let Some(id) = font_system.db().query(&query) else {
        return DecorationMetrics {
            underline_position: approx_ul_pos,
            underline_thickness: approx_ul_thick,
            strike_y: approx_strike_y,
            strike_thickness: approx_strike_thick,
        };
    };
    let Some(font) = font_system.get_font(id, Weight::NORMAL) else {
        return DecorationMetrics {
            underline_position: approx_ul_pos,
            underline_thickness: approx_ul_thick,
            strike_y: approx_strike_y,
            strike_thickness: approx_strike_thick,
        };
    };

    let m = font.metrics();
    let upem = f32::from(m.units_per_em).max(1.0);
    let scale = font_size / upem;

    let (underline_position, underline_thickness) = match m.underline {
        Some(ul) => (
            (-ul.offset * scale).max(1.0),
            (ul.thickness.abs() * scale).max(1.0),
        ),
        None => (approx_ul_pos, approx_ul_thick),
    };

    let (strike_y, strike_thickness) = if let Some(st) = m.strikeout {
        (
            (baseline_y - st.offset * scale).clamp(0.0, (cell_h - 1.0).max(0.0)),
            (st.thickness.abs() * scale).max(1.0),
        )
    } else if let Some(xh) = m.x_height {
        (
            (baseline_y - xh * scale * 0.5).clamp(0.0, (cell_h - 1.0).max(0.0)),
            approx_strike_thick,
        )
    } else {
        (approx_strike_y, approx_strike_thick)
    };

    DecorationMetrics {
        underline_position,
        underline_thickness,
        strike_y,
        strike_thickness,
    }
}

/// Mide `cell_w` con `monospace_width` activo (avance real entre columnas).
fn measure_cell_width(
    font_system: &mut FontSystem,
    metrics: Metrics,
    family: &str,
    guess: f32,
) -> f32 {
    let mut buf = glyphon::Buffer::new(font_system, metrics);
    buf.set_monospace_width(font_system, Some(guess));
    buf.set_hinting(font_system, Hinting::Enabled);
    buf.set_text(
        font_system,
        "MMMMMMMMMM",
        &glyphon::Attrs::new().family(resolve_family(family)),
        Shaping::Advanced,
        None,
    );
    buf.shape_until_scroll(font_system, false);
    if let Some(run) = buf.layout_runs().next() {
        if run.glyphs.len() >= 2 {
            let advance = run.glyphs[1].x - run.glyphs[0].x;
            if advance > 0.0 {
                return advance;
            }
        }
        if run.line_w > 0.0 {
            return run.line_w / 10.0;
        }
    }
    guess
}

/// Baseline vertical dentro de la celda (ascent centrado en `cell_h`).
fn measure_baseline_y(
    font_system: &mut FontSystem,
    metrics: Metrics,
    family: &str,
    cell_w: f32,
    cell_h: f32,
) -> f32 {
    let mut buf = glyphon::Buffer::new(font_system, metrics);
    buf.set_monospace_width(font_system, Some(cell_w));
    buf.set_hinting(font_system, Hinting::Enabled);
    buf.set_size(font_system, Some(cell_w), Some(cell_h));
    buf.set_text(
        font_system,
        "M",
        &glyphon::Attrs::new().family(resolve_family(family)),
        Shaping::Advanced,
        None,
    );
    buf.shape_until_scroll(font_system, false);
    buf.layout_runs()
        .next()
        .map(|run| run.line_y)
        .unwrap_or(metrics.font_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FontConfig;
    use crate::renderer::terminal_fallback::create_font_system;

    #[test]
    fn cell_geometry_is_integer_floor() {
        let mut fs = create_font_system();
        let fc = FontConfig::default();
        let m = CellMetrics::measure(
            &mut fs,
            &fc.family,
            fc.size as f32,
            fc.line_height,
            fc.glyph_offset,
            1.0,
        );
        assert_eq!(m.geometry.cell_w, m.cell_w.floor() as u32);
        assert_eq!(m.geometry.cell_h, m.cell_h.floor() as u32);
    }

    #[test]
    fn decoration_metrics_are_positive() {
        let mut fs = create_font_system();
        let fc = FontConfig::default();
        let m = CellMetrics::measure(
            &mut fs,
            &fc.family,
            fc.size as f32,
            fc.line_height,
            fc.glyph_offset,
            1.5,
        );
        assert!(m.underline_thickness >= 1.0);
        assert!(m.underline_position >= 1.0);
        assert!(m.strike_thickness >= 1.0);
        assert!(m.strike_y >= 0.0 && m.strike_y < m.cell_h);
        assert!((m.scale_factor - 1.5).abs() < f32::EPSILON);
    }
}
