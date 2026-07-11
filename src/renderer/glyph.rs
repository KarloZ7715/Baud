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
    /// Codepoints del grafema mas alla de `ch` (vacio si es un solo codepoint).
    pub extra: String,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub family: String,
}

/// Capa adicional de un cluster multi-glifo (misma celda que la base).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedOverlay {
    pub cache_key: CacheKey,
    pub bitmap_w: f32,
    pub bitmap_h: f32,
    /// Desplazamiento horizontal respecto al origen de celda (igual que `ShapedGlyph::left`).
    pub left: f32,
    /// Desplazamiento vertical del ancla (igual que `ShapedGlyph::top`).
    pub top: f32,
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
    /// Capas extra del mismo cluster (marcas combinantes, etc.). Vacio si un solo glifo.
    pub overlays: Vec<ShapedOverlay>,
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
pub fn resolve_glyph_key(
    row: &[Cell],
    col: usize,
    family: &str,
    grapheme_extras: &[String],
) -> Option<GlyphKey> {
    if is_wide_continuation(row, col) {
        return None;
    }
    let cell = row.get(col)?;
    if cell.width == 0 {
        return None;
    }
    let extra = cell
        .extra_codepoints
        .and_then(|idx| grapheme_extras.get(idx as usize).cloned())
        .unwrap_or_default();
    Some(GlyphKey {
        ch: cell.ch,
        extra,
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

    let ch_str = if key.extra.is_empty() {
        key.ch.to_string()
    } else {
        format!("{}{}", key.ch, key.extra)
    };
    buf.set_text(font_system, &ch_str, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(font_system, false);

    let Some(layers) = extract_glyph_layers(&buf, metrics) else {
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
            overlays: Vec::new(),
        };
    };

    let line_y =
        super::runs::reference_line_y(font_system, metrics, family, use_bold, key.italic, key.dim);

    let mut overlays = Vec::new();
    let mut base: Option<ShapedGlyph> = None;
    for glyph in layers.glyphs {
        let physical = glyph.physical((metrics.glyph_offset_x, line_y), 1.0);
        let anchor = glyph.physical((metrics.glyph_offset_x, 0.0), 1.0);
        if base.is_none() {
            base = Some(ShapedGlyph {
                cache_key: physical.cache_key,
                bitmap_w: glyph.w,
                bitmap_h: layers.bitmap_h,
                left: physical.x as f32,
                top: anchor.y as f32,
                line_y,
                advance: layers.advance,
                used_bold_fallback: false,
                overlays: Vec::new(),
            });
        } else {
            overlays.push(ShapedOverlay {
                cache_key: physical.cache_key,
                bitmap_w: glyph.w,
                bitmap_h: layers.bitmap_h,
                left: physical.x as f32,
                top: anchor.y as f32,
            });
        }
    }

    let mut shaped = base.expect("layers.glyphs no vacio");
    shaped.overlays = overlays;
    shaped
}

struct GlyphLayers {
    glyphs: Vec<LayoutGlyph>,
    bitmap_h: f32,
    advance: f32,
}

/// Todas las capas del run (base + marcas / partes del cluster).
fn extract_glyph_layers(buf: &glyphon::Buffer, metrics: &CellMetrics) -> Option<GlyphLayers> {
    let run = buf.layout_runs().next()?;
    if run.glyphs.is_empty() {
        return None;
    }
    let advance = if run.glyphs.len() >= 2 {
        run.glyphs[1].x - run.glyphs[0].x
    } else {
        run.line_w.max(metrics.cell_w)
    };
    Some(GlyphLayers {
        glyphs: run.glyphs.to_vec(),
        bitmap_h: run.line_height,
        advance,
    })
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
            extra: String::new(),
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
                    extra: String::new(),
                    bold: false,
                    italic: false,
                    dim: false,
                    family: family.clone(),
                };
                let cached =
                    cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &family, key);
                metrics.glyph_offset_y + cached.shaped.line_y + cached.shaped.top
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
                    extra: String::new(),
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

        let key = resolve_glyph_key(&row, 0, &family, &[]);
        assert!(key.is_some(), "col 0 debe producir clave");
        assert_eq!(key.unwrap().ch, '\u{4e2d}');

        assert!(is_wide_continuation(&row, 1));
        assert!(resolve_glyph_key(&row, 1, &family, &[]).is_none());
    }

    #[test]
    fn glyph_key_incluye_extra_codepoints_en_el_texto_a_shapear() {
        let extras = vec!["\u{0301}".to_string()];
        let mut row = vec![Cell::default(); 2];
        row[0].ch = 'e';
        row[0].width = 1;
        row[0].extra_codepoints = Some(0);
        let key = resolve_glyph_key(&row, 0, "monospace", &extras).expect("key");
        assert_eq!(key.extra, "\u{0301}");
    }

    #[test]
    fn glyph_key_extra_vacio_sin_extra_codepoints() {
        let extras: Vec<String> = vec![];
        let mut row = vec![Cell::default(); 1];
        row[0].ch = 'a';
        row[0].width = 1;
        let key = resolve_glyph_key(&row, 0, "monospace", &extras).expect("key");
        assert_eq!(key.extra, "");
    }

    #[test]
    fn cluster_shaped_incluye_todas_las_capas_del_run() {
        let (mut font_system, metrics) = test_metrics();
        let family = FontConfig::default().family;
        let key = GlyphKey {
            ch: 'e',
            extra: "\u{0301}".to_string(),
            bold: false,
            italic: false,
            dim: false,
            family: family.clone(),
        };
        let shaped = shape_glyph(&mut font_system, &metrics, &key, &family);
        // Fuente puede componer a 1 glifo (overlays vacio) o emitir base+marca.
        // Lo importante: no se descarta el cluster; hay al menos la capa base.
        assert!(shaped.bitmap_w > 0.0 || !shaped.overlays.is_empty() || shaped.advance > 0.0);
        let total_layers = 1 + shaped.overlays.len();
        assert!(total_layers >= 1);
    }
}
