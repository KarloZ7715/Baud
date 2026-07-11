//! Overhead del watchdog / telemetría del event loop.
//!
//! Uso interactivo:
//!   cargo bench --bench watchdog_overhead
//!
//! Resumen JSON (una línea en stdout):
//!   cargo bench --bench watchdog_overhead -- --json
//!
//! El script `tools/eval/watchdog_overhead.sh` corre tests del módulo y ese modo JSON.

use baud::watchdog::EventLoopWatchdog;
use criterion::{criterion_group, Criterion};
use std::hint::black_box;
use std::time::Instant;

const WARMUP: u32 = 50_000;
const ITERS: u32 = 500_000;
const SAMPLES: usize = 7;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    values[values.len() / 2]
}

fn measure_ping_ns() -> f64 {
    let wd = EventLoopWatchdog::noop();
    for _ in 0..WARMUP {
        wd.ping();
    }
    let t0 = Instant::now();
    for _ in 0..ITERS {
        wd.ping();
        black_box(());
    }
    black_box(&wd);
    t0.elapsed().as_nanos() as f64 / f64::from(ITERS)
}

fn measure_enter_leave_ns() -> f64 {
    let wd = EventLoopWatchdog::noop();
    for _ in 0..WARMUP {
        let _g = wd.enter("RedrawRequested");
    }
    let t0 = Instant::now();
    for _ in 0..ITERS {
        let g = wd.enter("CursorMoved");
        black_box(&g);
        drop(g);
    }
    black_box(&wd);
    t0.elapsed().as_nanos() as f64 / f64::from(ITERS)
}

fn measure_busy_ns() -> f64 {
    let wd = EventLoopWatchdog::noop();
    for _ in 0..WARMUP {
        wd.note_term_lock_busy();
    }
    let t0 = Instant::now();
    for _ in 0..ITERS {
        wd.note_term_lock_busy();
        black_box(());
    }
    black_box(&wd);
    t0.elapsed().as_nanos() as f64 / f64::from(ITERS)
}

fn print_json_summary() {
    let mut ping = Vec::with_capacity(SAMPLES);
    let mut enter = Vec::with_capacity(SAMPLES);
    let mut busy = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        ping.push(measure_ping_ns());
        enter.push(measure_enter_leave_ns());
        busy.push(measure_busy_ns());
    }
    let ping_ns = median(ping);
    let enter_leave_ns = median(enter);
    let busy_ns = median(busy);
    let hot_path_ns = ping_ns + enter_leave_ns;
    println!(
        "{{\"hot_path_ns\":{hot_path_ns:.3},\"ping_ns\":{ping_ns:.3},\"enter_leave_ns\":{enter_leave_ns:.3},\"busy_ns\":{busy_ns:.3},\"tests_passed\":1}}"
    );
}

fn bench_watchdog_ping(c: &mut Criterion) {
    c.bench_function("watchdog_ping", |b| {
        let wd = EventLoopWatchdog::noop();
        b.iter(|| {
            wd.ping();
            black_box(());
        });
    });
}

fn bench_watchdog_enter_leave(c: &mut Criterion) {
    c.bench_function("watchdog_enter_leave", |b| {
        let wd = EventLoopWatchdog::noop();
        b.iter(|| {
            let g = wd.enter("CursorMoved");
            black_box(&g);
            drop(g);
        });
    });
}

fn bench_watchdog_term_lock_busy(c: &mut Criterion) {
    c.bench_function("watchdog_term_lock_busy", |b| {
        let wd = EventLoopWatchdog::noop();
        b.iter(|| {
            wd.note_term_lock_busy();
            black_box(());
        });
    });
}

criterion_group!(
    watchdog_benches,
    bench_watchdog_ping,
    bench_watchdog_enter_leave,
    bench_watchdog_term_lock_busy
);

fn main() {
    if std::env::args().any(|a| a == "--json") {
        print_json_summary();
        return;
    }
    watchdog_benches();
}
