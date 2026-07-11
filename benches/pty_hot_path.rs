//! Benches de Criterion para el hot path de inbound del PTY en Linux.
//!
//! Nota de base: `inbound_coalesced_256k` mide spawn+drain de una carga fija.
//! Gate: sin regresión frente a esta bench tras cambios en el inbound;
//! la ruta de producción no debe hacer `to_vec` por cada chunk leído.

use std::hint::black_box;
use std::time::Duration;

use baud::pty::{spawn_with, ProcessConfig, SessionBackend};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

const PAYLOAD: usize = 256 * 1024;

fn spawn_writer() -> baud::pty::Pty {
    let cfg = ProcessConfig {
        shell: "bash".into(),
        args: vec!["-c".into(), format!("head -c {PAYLOAD} /dev/zero")],
        working_directory: None,
        env: Vec::new(),
        startup_command: None,
        login_shell: false,
    };
    spawn_with(&cfg).expect("spawn PTY para bench")
}

/// Réplica del coalesce de inbound del event_loop: llena `out` reutilizable
/// y toma ownership una sola vez.
fn drain_coalesced(master: &mut baud::pty::Pty) -> usize {
    let mut scratch = [0u8; 4096];
    let mut out = Vec::with_capacity(4096);
    let mut total = 0usize;
    loop {
        match master.read_output(&mut scratch) {
            Ok(0) => {
                total += out.len();
                break;
            }
            Ok(n) => {
                out.extend_from_slice(&scratch[..n]);
                if out.len() >= PAYLOAD {
                    total += out.len();
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                total += out.len();
                break;
            }
            Err(_) => {
                total += out.len();
                break;
            }
        }
    }
    total
}

fn bench_pty_inbound_coalesced(c: &mut Criterion) {
    let mut group = c.benchmark_group("pty_hot_path");
    group.throughput(Throughput::Bytes(PAYLOAD as u64));
    group.measurement_time(Duration::from_secs(8));
    group.bench_function("inbound_coalesced_256k", |b| {
        b.iter(|| {
            let mut master = spawn_writer();
            let n = drain_coalesced(&mut master);
            black_box(n);
            drop(master);
        });
    });
    group.finish();
}

fn bench_pty_write_echo(c: &mut Criterion) {
    c.bench_function("pty_write_echo_line", |b| {
        b.iter(|| {
            let mut master = spawn_with(&ProcessConfig {
                shell: "bash".into(),
                args: Vec::new(),
                working_directory: None,
                env: Vec::new(),
                startup_command: None,
                login_shell: false,
            })
            .expect("spawn");
            master.write_input(b"echo BENCH_OK\n").expect("write");
            let mut scratch = [0u8; 4096];
            let mut found = false;
            for _ in 0..200 {
                match master.read_output(&mut scratch) {
                    Ok(0) => break,
                    Ok(n) => {
                        if scratch[..n].windows(8).any(|w| w == b"BENCH_OK") {
                            found = true;
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            black_box(found);
            drop(master);
        });
    });
}

criterion_group!(benches, bench_pty_inbound_coalesced, bench_pty_write_echo);
criterion_main!(benches);
