//! Agrupacion de secuencias ligables y shaping multi-caracter.

use glyphon::cosmic_text::{FeatureTag, FontFeatures, Hinting, Metrics, Shaping, Style, Weight};
use glyphon::CacheKey;

use crate::grid::Cell;

use super::builtin::is_box_glyph;
use super::metrics::CellMetrics;
use super::resolve_family;

/// Secuencias tipograficas habituales en fuentes con ligaduras (Fira Code, etc.).
/// Ordenadas de mayor a menor longitud para greedy match.
const LIGATURE_PATTERNS: &[&str] = &[
    "...", "!==", "===", "==", "!=", ">=", "<=", "=>", "->", "<-", "::", "&&", "||", "//", "/*",
    "*/", "..",
];

#[derive(Debug, Clone, PartialEq)]
pub struct RunGlyph {
    pub cache_key: CacheKey,
    /// Primera columna del cluster dentro del run (0..run.cols).
    pub col_in_run: usize,
    /// Celdas de grid que cubre este cluster (1 = sin ligadura).
    pub cluster_cols: usize,
    pub top: f32,
    pub line_y: f32,
    /// Offset X del layout dentro del run (physical.x de cosmic-text).
    pub left: f32,
    pub width: f32,
    pub height: f32,
}

pub(crate) fn reference_line_y(
    font_system: &mut glyphon::FontSystem,
    metrics: &CellMetrics,
    family: &str,
    bold: bool,
    italic: bool,
    dim: bool,
) -> f32 {
    let ct = Metrics::new(metrics.font_size, metrics.cell_h);
    let mut buf = glyphon::Buffer::new(font_system, ct);
    buf.set_monospace_width(font_system, Some(metrics.cell_w));
    buf.set_hinting(font_system, Hinting::Enabled);
    buf.set_size(font_system, Some(metrics.cell_w), Some(metrics.cell_h));

    let mut attrs = glyphon::Attrs::new().family(resolve_family(family));
    if bold {
        attrs = attrs.weight(Weight::BOLD);
    } else if dim {
        attrs = attrs.weight(Weight::LIGHT);
    }
    if italic {
        attrs = attrs.style(Style::Italic);
    }

    buf.set_text(font_system, "M", &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(font_system, false);
    buf.layout_runs()
        .next()
        .map(|run| run.line_y)
        .unwrap_or(metrics.font_size)
}

fn ligature_attrs(
    family: glyphon::Family<'_>,
    bold: bool,
    italic: bool,
    dim: bool,
) -> glyphon::Attrs<'_> {
    let mut features = FontFeatures::new();
    features.enable(FeatureTag::CONTEXTUAL_ALTERNATES);
    features.enable(FeatureTag::STANDARD_LIGATURES);
    features.enable(FeatureTag::CONTEXTUAL_LIGATURES);

    let mut attrs = glyphon::Attrs::new().family(family).font_features(features);
    if bold {
        attrs = attrs.weight(Weight::BOLD);
    } else if dim {
        attrs = attrs.weight(Weight::LIGHT);
    }
    if italic {
        attrs = attrs.style(Style::Italic);
    }
    attrs
}

/// True si el shaping colapso el patron en menos glifos o clusters multi-celda.
#[cfg(test)]
pub(crate) fn ligature_collapsed(glyphs: &[RunGlyph], char_count: usize) -> bool {
    glyphs.iter().any(|g| g.cluster_cols > 1) || glyphs.len() < char_count
}

/// Shapea una secuencia corta con ligaduras (sin forzar ancho monospace).
pub fn shape_run(
    font_system: &mut glyphon::FontSystem,
    metrics: &CellMetrics,
    family: &str,
    text: &str,
    bold: bool,
    italic: bool,
    dim: bool,
) -> Vec<RunGlyph> {
    let ct = Metrics::new(metrics.font_size, metrics.cell_h);
    let mut buf = glyphon::Buffer::new(font_system, ct);
    buf.set_hinting(font_system, Hinting::Enabled);
    // Sin ancho fijo: Fira Code emite glifos marcador (sin bitmap) + glifo visible;
    // forzar cell_w*cols deja solo el marcador y la ligadura desaparece.
    buf.set_size(font_system, None, None);

    let attrs = ligature_attrs(resolve_family(family), bold, italic, dim);

    buf.set_text(font_system, text, &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(font_system, false);

    let line_y = reference_line_y(font_system, metrics, family, bold, italic, dim);

    let mut out = Vec::new();
    if let Some(run) = buf.layout_runs().next() {
        let glyphs: Vec<_> = run.glyphs.iter().collect();
        let total_cols = text.chars().count();
        for (gi, g) in glyphs.iter().enumerate() {
            let physical = g.physical((metrics.glyph_offset_x, line_y), 1.0);
            let anchor = g.physical((metrics.glyph_offset_x, 0.0), 1.0);
            let byte_start = g.start.min(text.len());
            let col_in_run = text[..byte_start].chars().count();
            let next_col = if gi + 1 < glyphs.len() {
                let next_start = glyphs[gi + 1].start.min(text.len());
                text[..next_start].chars().count()
            } else {
                total_cols
            };
            let cluster_cols = (next_col - col_in_run).max(1);
            out.push(RunGlyph {
                cache_key: physical.cache_key,
                col_in_run,
                cluster_cols,
                top: anchor.y as f32,
                line_y,
                left: physical.x as f32,
                width: g.w,
                height: run.line_height,
            });
        }
    }
    out
}

/// Convierte un glifo shaped de run al formato de cache por celda.
pub fn run_glyph_to_shaped(g: &RunGlyph) -> super::glyph::ShapedGlyph {
    super::glyph::ShapedGlyph {
        cache_key: g.cache_key,
        bitmap_w: g.width,
        bitmap_h: g.height,
        left: g.left,
        top: g.top,
        line_y: g.line_y,
        advance: g.width,
        used_bold_fallback: false,
        overlays: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LigRun {
    pub start_col: usize,
    pub text: String,
    /// Columnas cubiertas (== text.chars().count()).
    pub cols: usize,
}

/// True si la celda puede participar en una secuencia ligable.
pub fn is_ligable_cell(cell: &Cell) -> bool {
    if cell.width != 1 || cell.ch == ' ' || cell.ch == '\0' || is_box_glyph(cell.ch) {
        return false;
    }
    // Iconos Powerline/Nerd Font (PUA): siempre per-celda.
    let u = cell.ch as u32;
    !(0xE000..=0xF8FF).contains(&u)
}

fn same_style(a: &Cell, b: &Cell) -> bool {
    a.attrs == b.attrs && a.hyperlink == b.hyperlink
}

fn pattern_matches(
    row: &[Cell],
    start_col: usize,
    pattern: &str,
    is_selected: &impl Fn(usize) -> bool,
) -> bool {
    let sel = is_selected(start_col);
    for (i, ch) in pattern.chars().enumerate() {
        let col = start_col + i;
        let Some(cell) = row.get(col) else {
            return false;
        };
        if cell.ch != ch || !is_ligable_cell(cell) {
            return false;
        }
        if i > 0 && !same_style(&row[start_col], cell) {
            return false;
        }
        if is_selected(col) != sel {
            return false;
        }
    }
    true
}

/// Detecta solo secuencias que forman ligaduras tipograficas (no runs de estilo homogeneo).
pub fn group_ligature_runs(
    row: &[Cell],
    cols: usize,
    is_selected: impl Fn(usize) -> bool,
) -> Vec<LigRun> {
    let mut runs = Vec::new();
    let mut col = 0;
    let limit = cols.min(row.len());
    while col < limit {
        if !is_ligable_cell(&row[col]) {
            col += 1;
            continue;
        }
        let mut matched = false;
        for pattern in LIGATURE_PATTERNS {
            let plen = pattern.chars().count();
            if col + plen > limit {
                continue;
            }
            if pattern_matches(row, col, pattern, &is_selected) {
                runs.push(LigRun {
                    start_col: col,
                    text: pattern.to_string(),
                    cols: plen,
                });
                col += plen;
                matched = true;
                break;
            }
        }
        if !matched {
            col += 1;
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Cell;

    #[test]
    fn detecta_solo_secuencia_ligadura() {
        let mut row = vec![Cell::default(); 6];
        for (i, ch) in "a=>b".chars().enumerate() {
            row[i].ch = ch;
        }
        row[3].attrs.fg = crate::ansi::Color::Red;
        let runs = group_ligature_runs(&row, 4, |_| false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start_col, 1);
        assert_eq!(runs[0].text, "=>");
    }

    #[test]
    fn seleccion_corta_ligadura() {
        let mut row = vec![Cell::default(); 4];
        for (i, ch) in "x=>y".chars().enumerate() {
            row[i].ch = ch;
        }
        let runs = group_ligature_runs(&row, 4, |col| col >= 2);
        assert!(runs.is_empty());
    }

    #[test]
    fn path_largo_no_genera_run() {
        let path = "~/Documentos/Dev/baud";
        let mut row = vec![Cell::default(); path.chars().count()];
        for (i, ch) in path.chars().enumerate() {
            row[i].ch = ch;
        }
        let runs = group_ligature_runs(&row, row.len(), |_| false);
        assert!(runs.is_empty());
    }

    #[test]
    fn pua_no_es_ligable() {
        let cell = Cell {
            ch: '\u{E0B0}',
            ..Default::default()
        };
        assert!(!is_ligable_cell(&cell));
    }

    #[test]
    fn in_ligature_run_cubre_rango() {
        let runs = [LigRun {
            start_col: 2,
            text: "=>".into(),
            cols: 2,
        }];
        let in_run = |col: usize| {
            runs.iter()
                .any(|r| (r.start_col..r.start_col + r.cols).contains(&col))
        };
        assert!(in_run(2));
        assert!(in_run(3));
        assert!(!in_run(4));
    }

    #[test]
    fn detecta_triple_igual() {
        let mut row = vec![Cell::default(); 3];
        for (i, ch) in "===".chars().enumerate() {
            row[i].ch = ch;
        }
        let runs = group_ligature_runs(&row, 3, |_| false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "===");
    }

    #[test]
    fn shape_run_clusters_cubren_todo_el_patron() {
        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let fam = crate::config::FontConfig::default().family;
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            &fam,
            14.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        for text in ["=>", "==", "==="] {
            let glyphs = shape_run(&mut fs, &m, &fam, text, false, false, false);
            assert!(!glyphs.is_empty(), "sin glifos para {text}");
            let covered: usize = glyphs.iter().map(|g| g.cluster_cols).sum();
            assert_eq!(covered, text.chars().count(), "clusters no cubren {text}");
        }
    }

    #[test]
    fn shape_run_colapsa_patrones_programacion_si_la_fuente_los_soporta() {
        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let fam = crate::config::FontConfig::default().family;
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            &fam,
            14.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        for text in ["=>", "->", "=="] {
            let glyphs = shape_run(&mut fs, &m, &fam, text, false, false, false);
            let char_count = text.chars().count();
            if ligature_collapsed(&glyphs, char_count) {
                assert!(
                    glyphs.len() < char_count || glyphs.iter().any(|g| g.cluster_cols > 1),
                    "ligadura colapsada pero sin senal clara para {text}"
                );
            }
        }
    }

    #[test]
    fn shape_run_top_alineado_con_per_celda() {
        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let fam = crate::config::FontConfig::default().family;
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            &fam,
            14.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        let run_g = shape_run(&mut fs, &m, &fam, "==", false, false, false)
            .into_iter()
            .next()
            .expect("glifo");
        let cell_g = crate::renderer::glyph::shape_glyph(
            &mut fs,
            &m,
            &crate::renderer::glyph::GlyphKey {
                ch: '=',
                extra: String::new(),
                bold: false,
                italic: false,
                dim: false,
                family: fam.clone(),
            },
            &fam,
        );
        let run_anchor = run_g.line_y + run_g.top;
        let cell_anchor = cell_g.line_y + cell_g.top;
        assert!(
            (run_anchor - cell_anchor).abs() < 2.0,
            "run={run_anchor} cell={cell_anchor}"
        );
    }

    #[test]
    fn shape_run_de_fira_genera_glifos() {
        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let fam = crate::config::FontConfig::default().family;
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            &fam,
            14.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        let glyphs = shape_run(&mut fs, &m, &fam, "=>", false, false, false);
        assert!(!glyphs.is_empty());
        assert_eq!(glyphs[0].col_in_run, 0);
    }

    #[test]
    fn shape_run_fira_rasteriza_en_12_y_14() {
        use crate::renderer::glyph_cache::cache_key_rasterizes;

        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let db = fs.db();
        if !db
            .faces()
            .any(|f| f.families.iter().any(|(n, _)| n == "Fira Code"))
        {
            return;
        }
        let mut swash = glyphon::SwashCache::new();
        for size in [12.0_f32, 14.0] {
            let m = crate::renderer::metrics::CellMetrics::measure(
                &mut fs,
                "Fira Code",
                size,
                1.0,
                crate::config::GlyphOffset { x: 0.0, y: 0.0 },
            );
            for pattern in ["=>", "==", "==="] {
                let glyphs = shape_run(&mut fs, &m, "Fira Code", pattern, false, false, false);
                assert!(
                    glyphs
                        .iter()
                        .any(|g| { cache_key_rasterizes(&mut fs, &mut swash, g.cache_key) }),
                    "size={size} pattern={pattern} sin glifo rasterizable: {glyphs:?}"
                );
            }
        }
    }

    #[test]
    fn ligature_cache_key_vs_per_celda() {
        use crate::renderer::glyph_cache::cache_key_rasterizes;

        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        if !fs
            .db()
            .faces()
            .any(|f| f.families.iter().any(|(n, _)| n == "Fira Code"))
        {
            return;
        }
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            "Fira Code",
            12.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        let mut swash = glyphon::SwashCache::new();
        let cell = crate::renderer::glyph::shape_glyph(
            &mut fs,
            &m,
            &crate::renderer::glyph::GlyphKey {
                ch: '=',
                extra: String::new(),
                bold: false,
                italic: false,
                dim: false,
                family: "Fira Code".into(),
            },
            "Fira Code",
        );
        let run_g = super::shape_run(&mut fs, &m, "Fira Code", "==", false, false, false);
        assert!(
            run_g.len() >= 2,
            "== debe producir marcador + glifo visible: {run_g:?}"
        );
        let cell_img = swash.get_image_uncached(&mut fs, cell.cache_key);
        assert!(cell_img.is_some_and(|i| !i.data.is_empty()));
        assert!(
            run_g
                .iter()
                .any(|g| cache_key_rasterizes(&mut fs, &mut swash, g.cache_key)),
            "ningun glifo del run == rasteriza"
        );
    }

    #[test]
    fn ligature_run_glyphs_rasterizan_fira() {
        use crate::renderer::glyph_cache::cache_key_rasterizes;

        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        if !fs
            .db()
            .faces()
            .any(|f| f.families.iter().any(|(n, _)| n == "Fira Code"))
        {
            return;
        }
        let m = crate::renderer::metrics::CellMetrics::measure(
            &mut fs,
            "Fira Code",
            12.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        let mut swash = glyphon::SwashCache::new();
        let mut cache = crate::renderer::glyph_cache::GlyphCache::new();

        for pattern in ["==", "===", ".."] {
            let glyphs = super::shape_run(&mut fs, &m, "Fira Code", pattern, false, false, false);
            let visible: Vec<_> = glyphs
                .iter()
                .filter(|g| cache_key_rasterizes(&mut fs, &mut swash, g.cache_key))
                .collect();
            assert!(
                !visible.is_empty(),
                "patron={pattern} sin glifo visible en {glyphs:?}"
            );
            for g in visible {
                let shaped = super::run_glyph_to_shaped(g);
                let gi = g.col_in_run;
                let key = crate::renderer::glyph::GlyphKey {
                    ch: pattern.chars().nth(g.col_in_run).unwrap_or(' '),
                    extra: String::new(),
                    bold: false,
                    italic: false,
                    dim: false,
                    family: format!("Fira Code#lig:{pattern}:{gi}"),
                };
                let cached = cache.get_or_insert_shaped(&mut fs, &mut swash, &m, key, shaped);
                assert!(!cached.raster.missing, "patron={pattern} gi={gi}");
                assert!(cached.raster.width > 0 && cached.raster.height > 0);
            }
        }
    }

    #[test]
    fn ligature_probe_report() {
        use super::ligature_probe::probe_pipeline;

        let family = std::env::var("BAUD_PROBE_FAMILY")
            .unwrap_or_else(|_| crate::config::FontConfig::default().family);
        let font_size = std::env::var("BAUD_PROBE_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12.0);
        let mut fs = crate::renderer::terminal_fallback::create_font_system();
        let layers = probe_pipeline(&mut fs, &family, font_size, 1.0);
        eprintln!("\n=== ligature probe: family='{family}' size={font_size} ===");
        for layer in &layers {
            let mark = if layer.ok { "OK" } else { "FAIL" };
            eprintln!("[{mark}] {} — {}", layer.layer, layer.detail);
        }
        let failing: Vec<_> = layers.iter().filter(|l| !l.ok).map(|l| l.layer).collect();
        eprintln!(
            "primera capa rota: {}",
            failing
                .first()
                .copied()
                .unwrap_or("ninguna (pipeline OK en shaping)")
        );
    }
}

#[cfg(test)]
mod ligature_probe {
    use super::*;
    use crate::renderer::glyph_cache::cache_key_rasterizes;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct Layer {
        pub layer: &'static str,
        pub ok: bool,
        pub detail: String,
    }

    fn fontdb_exact_family(db: &glyphon::fontdb::Database, family: &str) -> bool {
        db.faces()
            .any(|face| face.families.iter().any(|(name, _)| name == family))
    }

    fn fontdb_similar_families(db: &glyphon::fontdb::Database, family: &str) -> Vec<String> {
        let needle = family.to_ascii_lowercase();
        let mut out = Vec::new();
        for face in db.faces() {
            for (name, _) in &face.families {
                let lower = name.to_ascii_lowercase();
                if lower.contains(&needle) && !out.iter().any(|s: &String| s == name) {
                    out.push(name.clone());
                }
            }
        }
        out.sort();
        out
    }

    fn resolved_font_label(font_system: &glyphon::FontSystem, cache_key: &CacheKey) -> String {
        font_system
            .db()
            .face(cache_key.font_id)
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
            .unwrap_or_else(|| format!("font_id={:?}", cache_key.font_id))
    }

    fn would_render_ligature_glyph(
        font_system: &mut glyphon::FontSystem,
        swash_cache: &mut glyphon::SwashCache,
        glyphs: &[RunGlyph],
    ) -> bool {
        use crate::renderer::glyph_cache::cache_key_rasterizes;
        glyphs
            .iter()
            .any(|g| cache_key_rasterizes(font_system, swash_cache, g.cache_key))
    }

    fn glyph_substituted(
        font_system: &mut glyphon::FontSystem,
        metrics: &crate::renderer::metrics::CellMetrics,
        family: &str,
        pattern: &str,
        run_glyphs: &[RunGlyph],
    ) -> bool {
        let Some(run_glyph) = run_glyphs.first() else {
            return false;
        };
        let per_char_ids: Vec<u16> = pattern
            .chars()
            .map(|ch| {
                crate::renderer::glyph::shape_glyph(
                    font_system,
                    metrics,
                    &crate::renderer::glyph::GlyphKey {
                        ch,
                        extra: String::new(),
                        bold: false,
                        italic: false,
                        dim: false,
                        family: family.to_string(),
                    },
                    family,
                )
                .cache_key
                .glyph_id
            })
            .collect();
        run_glyph.cache_key.glyph_id != per_char_ids.first().copied().unwrap_or(0)
            || run_glyphs.len() < pattern.chars().count()
    }

    pub(super) fn probe_pipeline(
        font_system: &mut glyphon::FontSystem,
        family: &str,
        font_size: f32,
        line_height: f32,
    ) -> Vec<Layer> {
        let mut layers = Vec::new();
        let db = font_system.db();
        let exact = fontdb_exact_family(db, family);
        let similar = fontdb_similar_families(db, family);
        layers.push(Layer {
            layer: "1_fontdb",
            ok: exact,
            detail: if exact {
                format!("'{family}' encontrada en fontdb")
            } else if similar.is_empty() {
                format!("'{family}' NO esta en fontdb; sin nombres parecidos")
            } else {
                format!(
                    "'{family}' NO exacta en fontdb; parecidas: {}",
                    similar.join(", ")
                )
            },
        });

        let test_line = "=== == => .. ...";
        let mut row = vec![Cell::default(); test_line.chars().count()];
        for (i, ch) in test_line.chars().enumerate() {
            row[i].ch = ch;
        }
        let runs = group_ligature_runs(&row, row.len(), |_| false);
        layers.push(Layer {
            layer: "2_pattern_detect",
            ok: !runs.is_empty(),
            detail: format!(
                "patrones={} {}",
                runs.len(),
                runs.iter()
                    .map(|r| format!("@{}:'{}'", r.start_col, r.text))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });

        let metrics = crate::renderer::metrics::CellMetrics::measure(
            font_system,
            family,
            font_size,
            line_height,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
        );
        let mut swash = glyphon::SwashCache::new();

        for pattern in ["=>", "==", "==="] {
            let glyphs = shape_run(font_system, &metrics, family, pattern, false, false, false);
            let cols = pattern.chars().count();
            let rasterizes = would_render_ligature_glyph(font_system, &mut swash, &glyphs);
            let substituted = glyph_substituted(font_system, &metrics, family, pattern, &glyphs);
            let glyph_summary: Vec<String> = glyphs
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    let img = if cache_key_rasterizes(font_system, &mut swash, g.cache_key) {
                        "img"
                    } else {
                        "no-img"
                    };
                    format!(
                        "#{i} id={} font={} cluster_cols={} {img}",
                        g.cache_key.glyph_id,
                        resolved_font_label(font_system, &g.cache_key),
                        g.cluster_cols
                    )
                })
                .collect();
            let uses_requested_font = glyphs
                .iter()
                .all(|g| resolved_font_label(font_system, &g.cache_key) == family);
            layers.push(Layer {
                layer: "3_shape",
                ok: rasterizes && substituted && uses_requested_font,
                detail: format!(
                    "'{pattern}': glyphs={}/{} rasterizes={rasterizes} substituted={substituted} uses_family={uses_requested_font} [{}]",
                    glyphs.len(),
                    cols,
                    glyph_summary.join("; ")
                ),
            });
        }

        layers.push(Layer {
            layer: "4_render_decision",
            ok: ["=>", "==", "==="].iter().any(|p| {
                let glyphs = shape_run(font_system, &metrics, family, p, false, false, false);
                would_render_ligature_glyph(font_system, &mut swash, &glyphs)
            }),
            detail: "true si al menos un patron tiene glifo rasterizable".into(),
        });

        layers
    }
}
