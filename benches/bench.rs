use baud::ansi::Term;
use baud::config::ThemeConfig;
use baud::grid::{Cell, DamageSnapshot, Grid};
use baud::renderer::{
    clear_builtin_cache, CellGeometry, CellMetrics, CellRenderer, ContrastCache, DisplayList,
    DisplayListBuilder, GlyphCache, GlyphStrings, Palette, RunShapeCache,
};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use glyphon::{FontSystem, SwashCache};

fn dummy_metrics() -> CellMetrics {
    CellMetrics {
        geometry: CellGeometry::from_u32(10, 20),
        cell_w: 10.0,
        cell_h: 20.0,
        font_size: 14.0,
        baseline_y: 14.0,
        underline_position: 1.0,
        underline_thickness: 1.0,
        scale_factor: 1.0,
        strike_y: 10.0,
        strike_thickness: 1.0,
        glyph_offset_x: 0.0,
        glyph_offset_y: 0.0,
        padding_x: 0.0,
        padding_y: 0.0,
    }
}

fn bench_scroll_push(c: &mut Criterion) {
    c.bench_function("scroll_push", |b| {
        let mut grid = Grid::new();
        b.iter(|| {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        });
    });
}

fn bench_scroll_pop(c: &mut Criterion) {
    c.bench_function("scroll_pop", |b| {
        let mut grid = Grid::new();
        // Llenar scrollback al maximo
        for _ in 0..baud::grid::MAX_SCROLLBACK {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        }
        b.iter(|| {
            grid.scroll_up_region(1, 0, grid.rows_count - 1);
        });
    });
}

fn bench_reflow(c: &mut Criterion) {
    c.bench_function("reflow", |b| {
        b.iter(|| {
            let mut grid = Grid::new();
            // Llenar con contenido antes de cada iteracion
            for row in 0..grid.rows_count {
                for col in 0..grid.cols_count {
                    grid.rows[row][col].ch = 'A';
                }
            }
            grid.reflow(40);
        });
    });
}

fn synthetic_grid(
    rows: usize,
    cols: usize,
) -> (Term, ThemeConfig, String, CellMetrics, Vec<Vec<Cell>>) {
    let term = Term::default();
    let theme = ThemeConfig::default();
    let metrics = dummy_metrics();
    let family = "monospace".to_string();

    let grid_rows: Vec<Vec<Cell>> = (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| {
                    Cell::with_ch(char::from_u32(b'A' as u32 + ((r + c) % 26) as u32).unwrap())
                })
                .collect()
        })
        .collect();

    (term, theme, family, metrics, grid_rows)
}

fn bench_display_list_build(c: &mut Criterion) {
    for (name, rows, cols) in [
        ("display_list_build_80x24", 24, 80),
        ("display_list_build_200x50", 50, 200),
    ] {
        let (term, theme, family, metrics, grid_rows) = synthetic_grid(rows, cols);
        let row_sources: Vec<&[Cell]> = grid_rows.iter().map(|r| r.as_slice()).collect();

        c.bench_function(name, |b| {
            let palette = Palette::from_theme(&theme);
            let mut contrast_cache = ContrastCache::default();
            let mut strings = baud::renderer::GlyphStrings::new();
            let mut run_shape_cache = RunShapeCache::new();
            b.iter(|| {
                let mut list = DisplayList::default();
                DisplayListBuilder::build(
                    &mut list,
                    &term,
                    &metrics,
                    &palette,
                    theme.dim_alpha,
                    &row_sources,
                    cols,
                    rows,
                    &family,
                    &DamageSnapshot::Full,
                    false,
                    true,
                    true,
                    true,
                    false,
                    &mut None,
                    &mut None,
                    &mut contrast_cache,
                    &mut strings,
                    &mut run_shape_cache,
                );
            });
        });
    }
}

/// Grid con contenido rico en secuencias de ligadura repetidas por fila, para
/// medir el camino de `font.ligatures = true` (shaping de runs, no solo
/// per-celda). Con cache de runs, el shaping real solo debe pagarse una vez
/// por combinacion distinta de familia/estilo/texto de run.
fn synthetic_ligature_grid(
    rows: usize,
    cols: usize,
    family: &str,
) -> (Term, ThemeConfig, CellMetrics, Vec<Vec<Cell>>) {
    let term = Term::default();
    let theme = ThemeConfig::default();
    let mut font_system = FontSystem::new();
    let metrics = CellMetrics::measure(
        &mut font_system,
        family,
        14.0,
        1.0,
        baud::config::GlyphOffset { x: 0.0, y: 0.0 },
        1.0,
    );

    let patterns = ["=>", "!==", "<=>", "->", "::", "..."];
    let grid_rows: Vec<Vec<Cell>> = (0..rows)
        .map(|r| {
            let mut row = vec![Cell::default(); cols];
            let mut col = 0;
            let mut i = 0;
            while col < cols {
                let pattern = patterns[(r + i) % patterns.len()];
                for ch in pattern.chars() {
                    if col >= cols {
                        break;
                    }
                    row[col].ch = ch;
                    col += 1;
                }
                if col < cols {
                    row[col].ch = ' ';
                    col += 1;
                }
                i += 1;
            }
            row
        })
        .collect();

    (term, theme, metrics, grid_rows)
}

fn bench_display_list_build_ligatures(c: &mut Criterion) {
    let (rows, cols) = (50, 200);
    let family = "monospace".to_string();
    let (term, theme, metrics, grid_rows) = synthetic_ligature_grid(rows, cols, &family);
    let row_sources: Vec<&[Cell]> = grid_rows.iter().map(|r| r.as_slice()).collect();

    c.bench_function("display_list_build_200x50_ligatures", |b| {
        let palette = Palette::from_theme(&theme);
        let mut contrast_cache = ContrastCache::default();
        let mut strings = GlyphStrings::new();
        let mut run_shape_cache = RunShapeCache::new();
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        b.iter(|| {
            let mut list = DisplayList::default();
            DisplayListBuilder::build(
                &mut list,
                &term,
                &metrics,
                &palette,
                theme.dim_alpha,
                &row_sources,
                cols,
                rows,
                &family,
                &DamageSnapshot::Full,
                false,
                true,
                true,
                true,
                true,
                &mut Some(&mut font_system),
                &mut Some(&mut swash_cache),
                &mut contrast_cache,
                &mut strings,
                &mut run_shape_cache,
            );
        });
    });
}

/// Mide `CellRenderer::build_custom_glyphs` con damage total (fila fria),
/// que es el costo de conversion completo que paga cada cache miss: 1
/// conversion de glifo cacheado por celda de texto, mas fondos/decoraciones.
/// No mide el camino de reuso por fila (cache caliente), que es O(1) por
/// frame sin damage y no aporta senal de regresion sobre la conversion.
fn bench_custom_glyphs_build(c: &mut Criterion) {
    let (rows, cols) = (50, 200);
    let (term, theme, family, metrics, grid_rows) = synthetic_grid(rows, cols);
    let row_sources: Vec<&[Cell]> = grid_rows.iter().map(|r| r.as_slice()).collect();
    let palette = Palette::from_theme(&theme);
    let mut contrast_cache = ContrastCache::default();
    let mut strings = GlyphStrings::new();
    let mut run_shape_cache = RunShapeCache::new();

    let mut list = DisplayList::default();
    DisplayListBuilder::build(
        &mut list,
        &term,
        &metrics,
        &palette,
        theme.dim_alpha,
        &row_sources,
        cols,
        rows,
        &family,
        &DamageSnapshot::Full,
        false,
        true,
        true,
        true,
        false,
        &mut None,
        &mut None,
        &mut contrast_cache,
        &mut strings,
        &mut run_shape_cache,
    );

    c.bench_function("custom_glyphs_build_200x50", |b| {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut glyph_cache = GlyphCache::new();
        let mut row_cache = Vec::new();
        let mut out = Vec::new();
        b.iter(|| {
            CellRenderer::build_custom_glyphs(
                &list,
                &metrics,
                &palette,
                theme.dim_alpha,
                &mut glyph_cache,
                &mut strings,
                &mut font_system,
                &mut swash_cache,
                &mut contrast_cache,
                &mut row_cache,
                &DamageSnapshot::Full,
                &mut out,
                true,
            )
            .expect("build_custom_glyphs");
        });
    });
}

fn bench_builtin_render_cached(c: &mut Criterion) {
    clear_builtin_cache();
    c.bench_function("builtin_render_cached", |b| {
        b.iter(|| {
            let _ = baud::renderer::box_mask('\u{2502}', 10, 20);
        });
    });
}

const PARSE_THROUGHPUT_TARGET_LEN: usize = 4 * 1024 * 1024;

/// Linea de 74 bytes tipo `ls -l`, terminada en `terminator`.
fn plain_line_74(terminator: u8) -> Vec<u8> {
    let base = b"-rwxr-xr-x  1 user user  8192 Jul 30 12:00 build/target/release/baud";
    let mut line = base.to_vec();
    line.resize(73, b' ');
    line.push(terminator);
    line
}

fn repeat_to_len(chunk: &[u8], target_len: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(target_len + chunk.len());
    while data.len() < target_len {
        data.extend_from_slice(chunk);
    }
    data
}

fn sgr_line() -> Vec<u8> {
    let mut line = Vec::new();
    line.extend_from_slice(
        b"\x1b[0m\x1b[32m-rwxr-xr-x\x1b[0m  1 \x1b[34muser\x1b[0m user  8192 Jul 30 12:00 baud\n",
    );
    line
}

fn unicode_line() -> Vec<u8> {
    // Mezcla de CJK, acento combinante y emoji con ZWJ (mujer + laptop).
    "你好世界 e\u{0301} \u{1F469}\u{200D}\u{1F4BB}\n"
        .as_bytes()
        .to_vec()
}

/// Grupo de benches del camino de parseo (`vte::Parser` + `Term::print`).
/// Cada iteracion crea un `Term` nuevo para que el scrollback no se llene
/// y desvie la medicion hacia otra cosa.
fn bench_parse_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_throughput");

    let plain_scroll = repeat_to_len(&plain_line_74(b'\n'), PARSE_THROUGHPUT_TARGET_LEN);
    group.throughput(Throughput::Bytes(plain_scroll.len() as u64));
    group.bench_function("parse_plain_scroll", |b| {
        b.iter(|| {
            let mut term = Term::new_sized(50, 200, 10_000);
            let mut parser = vte::Parser::new();
            parser.advance(&mut term, &plain_scroll);
        });
    });

    let plain_no_scroll = repeat_to_len(&plain_line_74(b'\r'), PARSE_THROUGHPUT_TARGET_LEN);
    group.throughput(Throughput::Bytes(plain_no_scroll.len() as u64));
    group.bench_function("parse_plain_no_scroll", |b| {
        b.iter(|| {
            let mut term = Term::new_sized(50, 200, 10_000);
            let mut parser = vte::Parser::new();
            parser.advance(&mut term, &plain_no_scroll);
        });
    });

    let newlines = repeat_to_len(b"\n", PARSE_THROUGHPUT_TARGET_LEN);
    group.throughput(Throughput::Bytes(newlines.len() as u64));
    group.bench_function("parse_newlines", |b| {
        b.iter(|| {
            let mut term = Term::new_sized(50, 200, 10_000);
            let mut parser = vte::Parser::new();
            parser.advance(&mut term, &newlines);
        });
    });

    let sgr = repeat_to_len(&sgr_line(), PARSE_THROUGHPUT_TARGET_LEN);
    group.throughput(Throughput::Bytes(sgr.len() as u64));
    group.bench_function("parse_sgr", |b| {
        b.iter(|| {
            let mut term = Term::new_sized(50, 200, 10_000);
            let mut parser = vte::Parser::new();
            parser.advance(&mut term, &sgr);
        });
    });

    let unicode = repeat_to_len(&unicode_line(), PARSE_THROUGHPUT_TARGET_LEN);
    group.throughput(Throughput::Bytes(unicode.len() as u64));
    group.bench_function("parse_unicode", |b| {
        b.iter(|| {
            let mut term = Term::new_sized(50, 200, 10_000);
            let mut parser = vte::Parser::new();
            parser.advance(&mut term, &unicode);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_scroll_push,
    bench_scroll_pop,
    bench_reflow,
    bench_display_list_build,
    bench_display_list_build_ligatures,
    bench_custom_glyphs_build,
    bench_builtin_render_cached,
    bench_parse_throughput
);
criterion_main!(benches);
