use baud::grid::Grid;
use criterion::{criterion_group, criterion_main, Criterion};

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

criterion_group!(benches, bench_scroll_push, bench_scroll_pop, bench_reflow);
criterion_main!(benches);
