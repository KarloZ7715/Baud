use baud::ansi::Term;
use baud::config::ThemeConfig;
use baud::grid::{Cell, DamageSnapshot, Grid};
use baud::renderer::{
    clear_builtin_cache, CellGeometry, CellMetrics, DisplayList, DisplayListBuilder, Palette,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn dummy_metrics() -> CellMetrics {
    CellMetrics {
        geometry: CellGeometry::from_u32(10, 20),
        cell_w: 10.0,
        cell_h: 20.0,
        font_size: 14.0,
        baseline_y: 14.0,
        underline_position: 1.0,
        underline_thickness: 1.0,
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
                .map(|c| Cell {
                    ch: char::from_u32(b'A' as u32 + ((r + c) % 26) as u32).unwrap(),
                    ..Default::default()
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
                );
            });
        });
    }
}

fn bench_builtin_render_cached(c: &mut Criterion) {
    clear_builtin_cache();
    c.bench_function("builtin_render_cached", |b| {
        b.iter(|| {
            let _ = baud::renderer::box_mask('\u{2502}', 10, 20);
        });
    });
}

criterion_group!(
    benches,
    bench_scroll_push,
    bench_scroll_pop,
    bench_reflow,
    bench_display_list_build,
    bench_builtin_render_cached
);
criterion_main!(benches);
