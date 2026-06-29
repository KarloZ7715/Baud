//! Resolucion y shaping de glifos por celda.

use glyphon::cosmic_text::{FontSystem, Hinting, LayoutGlyph, Metrics, Shaping, Style, Weight};
use glyphon::CacheKey;

use crate::grid::Cell;

use super::metrics::CellMetrics;
use super::resolve_family;

/// Clave de cache para un glifo con estilo tipografico.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub ch: char,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub family: String,
}

/// Glifo shaped listo para rasterizar (posicion relativa a origen de celda).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedGlyph {
    pub cache_key: CacheKey,
    pub bitmap_w: f32,
    pub bitmap_h: f32,
    /// Desplazamiento horizontal del bitmap respecto al origen de celda.
    pub left: f32,
    /// Desplazamiento vertical del ancla del glifo (sin line_y; ver `line_y`).
    pub top: f32,
    /// `run.line_y` de cosmic-text para la formula de posicion de glyphon.
    pub line_y: f32,
    /// Avance horizontal del glifo (para celdas anchas).
    pub advance: f32,
    /// True si se pidio bold pero la fuente no lo tenia.
    pub used_bold_fallback: bool,
}

/// True si `col` es la segunda (o posterior) columna de un glifo ancho.
pub fn is_wide_continuation(row: &[Cell], col: usize) -> bool {
    if col == 0 || col >= row.len() {
        return false;
    }
    for start in (0..col).rev() {
        if start >= row.len() {
            continue;
        }
        let w = row[start].width as usize;
        if w <= 1 {
            continue;
        }
        if start + w > col {
            return true;
        }
        break;
    }
    false
}

/// Construye la clave de cache para la celda en `(row, col)`.
///
/// Devuelve `None` en columnas de continuacion de glifos anchos o celdas vacias.
pub fn resolve_glyph_key(row: &[Cell], col: usize, family: &str) -> Option<GlyphKey> {
    if is_wide_continuation(row, col) {
        return None;
    }
    let cell = row.get(col)?;
    if cell.width == 0 {
        return None;
    }
    Some(GlyphKey {
        ch: cell.ch,
        bold: cell.attrs.bold,
        italic: cell.attrs.italic,
        dim: cell.attrs.dim,
        family: family.to_string(),
    })
}

/// Shapea un unico caracter con cosmic-text.
pub fn shape_glyph(
    font_system: &mut FontSystem,
    metrics: &CellMetrics,
    key: &GlyphKey,
    family: &str,
) -> ShapedGlyph {
    if key.bold {
        let bold = shape_with_style(font_system, metrics, key, family, true);
        if is_bold_weight(bold.cache_key.font_weight) {
            return bold;
        }
        let mut regular = shape_with_style(font_system, metrics, key, family, false);
        regular.used_bold_fallback = true;
        return regular;
    }
    shape_with_style(font_system, metrics, key, family, false)
}

fn is_bold_weight(weight: Weight) -> bool {
    weight.0 >= Weight::BOLD.0
}

fn shape_with_style(
    font_system: &mut FontSystem,
    metrics: &CellMetrics,
    key: &GlyphKey,
    family: &str,
    use_bold: bool,
) -> ShapedGlyph {
    let ct_metrics = Metrics::new(metrics.font_size, metrics.cell_h);
    let mut buf = glyphon::Buffer::new(font_system, ct_metrics);
    buf.set_monospace_width(font_system, Some(metrics.cell_w));
    buf.set_hinting(font_system, Hinting::Enabled);
    buf.set_size(font_system, Some(metrics.cell_w), Some(metrics.cell_h));

    let mut attrs = glyphon::Attrs::new().family(resolve_family(family));
    if use_bold {
        attrs = attrs.weight(Weight::BOLD);
    } else if key.dim {
        attrs = attrs.weight(Weight::LIGHT);
    }
    if key.italic {
        attrs = attrs.style(Style::Italic);
    }

    let ch_str = key.ch.to_string();
    buf.set_text(font_system, &ch_str, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(font_system, false);

    let (glyph, line_y, bitmap_h, advance) = match extract_glyph_layout(&buf, metrics) {
        Some(layout) => layout,
        None => {
            return ShapedGlyph {
                cache_key: CacheKey::new(
                    glyphon::fontdb::ID::dummy(),
                    0,
                    metrics.font_size,
                    (metrics.glyph_offset_x, metrics.glyph_offset_y),
                    Weight::NORMAL,
                    glyphon::cosmic_text::CacheKeyFlags::empty(),
                )
                .0,
                bitmap_w: metrics.cell_w,
                bitmap_h: metrics.cell_h,
                left: 0.0,
                top: 0.0,
                line_y: 0.0,
                advance: metrics.cell_w,
                used_bold_fallback: false,
            };
        }
    };

    // cosmic-text: ancla fija dentro de celda para cache_key coherente con grid
    let physical = glyph.physical((metrics.glyph_offset_x, line_y), 1.0);
    let anchor = glyph.physical((metrics.glyph_offset_x, 0.0), 1.0);

    ShapedGlyph {
        cache_key: physical.cache_key,
        bitmap_w: glyph.w,
        bitmap_h,
        left: physical.x as f32,
        top: anchor.y as f32,
        line_y,
        advance,
        used_bold_fallback: false,
    }
}

fn extract_glyph_layout(
    buf: &glyphon::Buffer,
    metrics: &CellMetrics,
) -> Option<(LayoutGlyph, f32, f32, f32)> {
    let run = buf.layout_runs().next()?;
    let glyph = run.glyphs.first()?.clone();
    let line_y = run.line_y;
    let advance = if run.glyphs.len() >= 2 {
        run.glyphs[1].x - run.glyphs[0].x
    } else {
        run.line_w.max(metrics.cell_w)
    };
    Some((glyph, line_y, run.line_height, advance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FontConfig;
    use crate::grid::Cell;

    use super::super::terminal_fallback::create_font_system;

    fn test_metrics() -> (glyphon::FontSystem, CellMetrics) {
        let mut font_system = create_font_system();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        );
        (font_system, metrics)
    }

    #[test]
    fn m_advance_equals_cell_w() {
        let (mut font_system, metrics) = test_metrics();
        let family = FontConfig::default().family;
        let key = GlyphKey {
            ch: 'M',
            bold: false,
            italic: false,
            dim: false,
            family: family.clone(),
        };
        let shaped = shape_glyph(&mut font_system, &metrics, &key, &family);
        assert!(
            (shaped.advance - metrics.cell_w).abs() < 0.5,
            "advance {} debe coincidir con cell_w {}",
            shaped.advance,
            metrics.cell_w
        );
    }

    #[test]
    fn holaesto_chars_share_glyph_anchor() {
        let (mut font_system, metrics) = test_metrics();
        let mut swash_cache = glyphon::SwashCache::new();
        let family = FontConfig::default().family;
        let mut cache = super::super::glyph_cache::GlyphCache::new();

        let anchors: Vec<f32> = "holaesto"
            .chars()
            .map(|ch| {
                let key = GlyphKey {
                    ch,
                    bold: false,
                    italic: false,
                    dim: false,
                    family: family.clone(),
                };
                let cached =
                    cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &family, key);
                metrics.glyph_offset_y + cached.shaped.line_y.round() + cached.shaped.top
            })
            .collect();

        let first = anchors[0];
        for (i, anchor) in anchors.iter().enumerate() {
            assert!(
                (anchor - first).abs() < 0.5,
                "glyph {i} anchor {anchor} debe coincidir con {first}"
            );
        }
    }

    #[test]
    fn same_row_chars_share_baseline_top() {
        let (mut font_system, metrics) = test_metrics();
        let family = FontConfig::default().family;
        let mut swash_cache = glyphon::SwashCache::new();
        let mut cache = super::super::glyph_cache::GlyphCache::new();
        let anchors: Vec<f32> = "baud"
            .chars()
            .map(|ch| {
                let key = GlyphKey {
                    ch,
                    bold: false,
                    italic: false,
                    dim: false,
                    family: family.clone(),
                };
                let cached =
                    cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &family, key);
                metrics.glyph_offset_y + cached.shaped.line_y.round() + cached.shaped.top
            })
            .collect();
        let first = anchors[0];
        for (i, anchor) in anchors.iter().enumerate() {
            assert!(
                (anchor - first).abs() < 0.5,
                "glyph {i} anchor {anchor} debe alinearse con {first}"
            );
        }
    }

    #[test]
    fn is_wide_continuation_safe_on_short_row() {
        let row = vec![Cell::default(); 10];
        assert!(!is_wide_continuation(&row, 10));
        assert!(!is_wide_continuation(&row, 100));
    }

    #[test]
    fn wide_char_continuation_returns_none() {
        let family = FontConfig::default().family;
        let mut row = vec![Cell::default(); 4];
        row[0].ch = '\u{4e2d}';
        row[0].width = 2;

        let key = resolve_glyph_key(&row, 0, &family);
        assert!(key.is_some(), "col 0 debe producir clave");
        assert_eq!(key.unwrap().ch, '\u{4e2d}');

        assert!(is_wide_continuation(&row, 1));
        assert!(resolve_glyph_key(&row, 1, &family).is_none());
    }
}
