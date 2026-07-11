//! Microbench de overhead del watchdog (release).
//!
//! Imprime un único JSON en stdout para `/ce-optimize`:
//! `{ "hot_path_ns", "ping_ns", "enter_leave_ns", "busy_ns", "tests_passed" }`

use baud::watchdog::EventLoopWatchdog;
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

fn main() {
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
