```yaml
titulo: "Estrategia de Testing , Detalle Operativo"
tipo: especificacion
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-14"
fecha_modificacion: "2026-06-14"
version: "0.1.0"
estado: borrador
tags: [testing, vttest, proptest, criterion, ci, operativo]
```

# Estrategia de Testing , Detalle Operativo

## 1. Resumen

Este documento complementa `docs/decisions/ADR-0006-
testing-strategy.md` con el detalle operativo de la
estrategia de testing: donde vive cada tipo de test,
como se ejecutan localmente, y el workflow exacto de
CI. La decision de alto nivel (4 niveles de testing) vive
en el ADR-0006; este doc se enfoca en la implementación.

## 2. Filosofia de Testing

Tres principios guian la estrategia:

1. **Los unit tests viven con el código.** Cada
   archivo `.rs` de domain tiene su `#[cfg(test)] mod
   tests`. Esto facilita encontrar tests cuando se
   modifica el código.

2. **Los integration tests reflejan casos de uso.** Los
   tests en `tests/` validan flujos end-to-end (arranque
   de bash, envio de input, lectura de output) que no
   son unidades atomicas.

3. **Los benchmarks miden antes de optimizar.** Cualquier
   optimizacion de performance debe ir acompanada de
   un benchmark que demuestre la mejora. Sin benchmark,
   la optimizacion es especulativa.

## 3. Unit Tests

### 3.1 Donde Viven

Unit tests por modulo:

- `src/grid/tests.rs`: ring buffer, scroll, reflow,
  resize, cursor.
- `src/parser/tests.rs`: secuencias CSI basicas, OSC,
  errores, edge cases.
- `src/cursor/tests.rs`: posición, wrap, save/restore
  (DECSC/DECRC), pending wrap.
- `src/selection/tests.rs`: 4 modos de selección,
  interseccion con grid.
- `src/config/tests.rs`: parseo de TOML, validacion,
  defaults.

Patron de organizacion:

```rust
// src/grid/mod.rs

pub mod cell;
pub mod cursor;
pub mod resize;
pub mod row;
pub mod selection;
pub mod storage;

#[cfg(test)]
mod tests;
```

```rust
// src/grid/tests.rs

use super::*;

#[test]
fn test_ring_buffer_rotate_up() {
    let mut storage: Storage<Row> = Storage::new(...);
    storage.rotate_up(5);
    assert_eq!(storage.zero, 5);
}

#[test]
fn test_pending_wrap_clears_on_cursor_move() {
    let mut grid = Grid::new(80, 24);
    grid.set_pending_wrap(true);
    grid.cursor_move(0, -1);
    assert!(!grid.pending_wrap());
}
```

### 3.2 Convenciones de Naming

- `test_<funcionalidad>_<condicion>` para casos
  positivos.
- `test_<funcionalidad>_<edge_case>` para edge cases.
- `test_<funcionalidad>_<error>` para casos de error.

Ejemplos:
- `test_resize_preserves_visible_content`
- `test_resize_to_zero_columns_panics`
- `test_scroll_up_with_invalid_region_ignores`

### 3.3 Helpers Comunes

En `src/test_utils.rs` (modulo privado, solo
compilable en tests):

```rust
#[cfg(test)]
pub fn make_grid(cols: usize, rows: usize) -> Grid { ... }

#[cfg(test)]
pub fn make_term_with_grid(cols: usize, rows: usize) -> Term<TestBackend> { ... }

#[cfg(test)]
pub fn assert_grid_eq(grid: &Grid, expected: &[&str]) { ... }
```

## 4. Integration Tests

### 4.1 Estructura de Archivos

```text
tests/
  common/
    mod.rs                # Helpers compartidos
    pty_helper.rs         # Setup de PTY para tests
  bash_startup.rs         # IT-001: bash arranca correctamente
  input_output.rs         # IT-002: input y output basicos
  resize.rs               # IT-003: SIGWINCH funciona
  alternate_screen.rs     # IT-004: vim/htop usan alt screen
  shutdown.rs             # IT-005: cierre limpio
  vttest/
    menu.expect           # Script expect para automatizar vttest
    run.sh                # Script que ejecuta vttest y parsea output
```

### 4.2 Casos de Test Clave

**IT-001: bash_startup**

```rust
#[test]
fn bash_starts_and_shows_prompt() {
    let (mut pty, parser) = make_test_pty("/bin/bash", &["--login"]);
    pty.spawn().expect("bash should start");
    let output = read_with_timeout(&pty, Duration::from_millis(500));
    assert!(output.contains("$") || output.contains(">"),
            "prompt should appear, got: {:?}", output);
}
```

**IT-002: input_output**

```rust
#[test]
fn echo_command_produces_output() {
    let (mut pty, parser) = make_test_pty("/bin/bash", &["--login"]);
    pty.spawn().expect("bash should start");
    wait_for_prompt(&pty);

    pty.write(b"echo hola\n").unwrap();
    let output = read_with_timeout(&pty, Duration::from_millis(500));
    assert!(output.contains("hola"),
            "should see hola, got: {:?}", output);
}
```

**IT-003: resize**

```rust
#[test]
fn resize_updates_columns_and_lines_env() {
    let (mut pty, parser) = make_test_pty("/bin/bash", &["--login"]);
    pty.spawn().expect("bash should start");
    wait_for_prompt(&pty);

    pty.write(b"echo $COLUMNS $LINES\n").unwrap();
    let before = read_with_timeout(&pty, Duration::from_millis(500));

    pty.resize(100, 30).expect("resize should succeed");
    pty.write(b"echo $COLUMNS $LINES\n").unwrap();
    let after = read_with_timeout(&pty, Duration::from_millis(500));

    assert!(after.contains("100 30"),
            "should see new size, got: {:?}", after);
}
```

**IT-004: alternate_screen**

```rust
#[test]
fn vim_uses_alternate_screen() {
    let (mut pty, parser) = make_test_pty("vim", &[]);
    pty.spawn().expect("vim should start");
    let initial = read_with_timeout(&pty, Duration::from_millis(500));

    pty.write(b"iHola mundo\x1b").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    pty.write(b":q!\n").unwrap();
    let final_state = read_with_timeout(&pty, Duration::from_millis(500));

    // El contenido de vim no debe contaminar el scrollback
    assert!(!final_state.contains("Hola mundo"));
}
```

### 4.3 Helpers de PTY

`tests/common/pty_helper.rs`:

```rust
pub fn make_test_pty(prog: &str, args: &[&str]) -> (Box<dyn MasterPty>, vte::Parser) {
    let pty = nix::pty::openpty().expect("openpty");
    let mut cmd = Command::new(prog);
    cmd.args(args);
    unsafe { cmd.pre_exec(|| { ... }) };
    cmd.spawn().expect("child should spawn");
    (Box::new(pty.master), vte::Parser::new())
}

pub fn read_with_timeout(pty: &dyn MasterPty, timeout: Duration) -> String {
    let mut buf = [0u8; 4096];
    let mut acc = Vec::new();
    let start = Instant::now();
    while start.elapsed() < timeout {
        match pty.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => acc.extend_from_slice(&buf[..n]),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    String::from_utf8_lossy(&acc).to_string()
}
```

## 5. Property-Based Testing (proptest)

### 5.1 Invariantes del Grid

Las invariantes que se verifican con proptest:

1. **Scroll preserva contenido total.** Hacer scroll
   up N lineas y luego scroll down N lineas
   devuelve el grid al estado original.
2. **Wrap se cancela con cursor move.** Activar wrap
   con pending_wrap=true y luego hacer CSI A (cursor
   up) cancela el wrap.
3. **Reflow preserva el contenido total.** Reflow de
   ancho W1 a W2 y luego a W1 preserva todos los
   caracteres (modulo lineas que exceden el nuevo
   ancho).
4. **DECSTBM respeta margenes.** Operaciones de
   scroll dentro de la región solo afectan lineas
   entre top y bottom.
5. **Save/restore es identidad.** DECSC seguido de
   mutaciones y luego DECRC devuelve el cursor al
   estado guardado.

### 5.2 Ejemplo de Estrategia

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn scroll_preserves_total_content(
        cols in 1u16..200,
        rows in 1u16..100,
        content in prop::collection::vec(any::<char>(), 0..1000),
        scroll_amount in 0u16..50,
    ) {
        let mut grid = Grid::new(cols as usize, rows as usize);
        grid.write(&content);

        let before = grid.total_chars();
        grid.scroll_up(scroll_amount as usize);
        grid.scroll_down(scroll_amount as usize);
        let after = grid.total_chars();

        prop_assert_eq!(before, after);
    }
}
```

### 5.3 Limitaciones

- Las estrategias no cubren combinaciones raras
  (ej: reflow + DECSTBM + scroll simultaneo). Estas
  se prueban con unit tests especificos.
- proptest se enfoca en el grid y el parser, no en
  el renderer (que requiere GPU).

## 6. vttest y esctest

### 6.1 vttest Manual

vttest (https://invisible-island.net/vttest/) se
ejecuta manualmente en cada milestone:

- **Despues de Fase 1:** categoría 1 (cursor movement).
- **Despues de Fase 2:** categoría 2 (screen features).
- **Despues de Fase 3:** categorías 1, 2, 6 (VT102).
- **Despues de Fase 5:** categorías 1-11 completas.

Resultado se reporta en `tests/vttest/RESULTS.md`:

```markdown
# vttest Results , iter 6

Fecha: 2026-06-14
Version: 0.1.0
Plataforma: Linux x86_64

## Categoria 1: Cursor Movement , 10/10 PASS

- Basic cursor motion: PASS
- Cursor up at top: PASS
- Cursor down at bottom: PASS
- ...

## Categoria 2: Screen Features , 8/8 PASS

- ...
```

### 6.2 esctest Automatico

esctest (https://github.com/MarcusJohnson91/esctest)
se ejecuta en CI en cada PR. Tests relevantes:

- `esctest/tests/esctest.test_dec_rqm.py`
- `esctest/tests/esctest.test_decstbm.py`
- `esctest/tests/esctest.test_decawm.py`

esctest se conecta al emulador como si fuera un
terminal, envia secuencias, y valida las respuestas
(DSR, DA1, DECRQM).

### 6.3 Automatizacion de vttest (Fase 5)

vttest es interactivo (menu). Para automatizarlo:

- `tests/vttest/menu.expect`: script `expect` que
  envia respuestas al menu.
- `tests/vttest/run.sh`: ejecuta vttest dentro de
  `expect`, captura output, parsea resultados.
- CI corre `run.sh` solo cuando hay cambios en
  `src/parser/` o `src/grid/`.

## 7. Benchmarks con criterion

### 7.1 Estructura

```text
benches/
  parser_throughput.rs   # Parser vte bytes/segundo
  scroll_latency.rs      # Scroll de N lineas
  render_time.rs         # Render de frame (con mock GPU)
  resize_time.rs         # Resize del grid
  criterion_harness.rs   # Helpers compartidos
```

### 7.2 Ejemplo de Benchmark

```rust
// benches/parser_throughput.rs

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use baud::parser::Parser;

fn bench_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");
    for size in [1024, 10240, 102400, 1048576].iter() {
        let input = generate_ansi_output(*size);
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &input,
            |b, input| {
                let mut parser = Parser::new();
                let mut term = make_test_term();
                b.iter(|| {
                    for byte in input {
                        parser.advance(&mut term, *byte);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_parser);
criterion_main!(benches);
```

### 7.3 Configuracion de CI

`.github/workflows/bench.yml`:

```yaml
name: Bench
on:
  push:
    branches: [main]
  pull_request:

jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-criterion
      - run: cargo criterion --message-format=json | tee bench-output.json
      - uses: criterion-graphs/criterion-graphs-action@v1
        with:
          output: bench-output.json
      - name: Detect regression
        run: |
          cargo criterion --bench parser_throughput -- --output-format bencher | \
            tee current.txt
          if regression_detected current.txt; then
            echo "::warning::Performance regression detected"
          fi
```

## 8. CI con GitHub Actions

### 8.1 Workflow Principal

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.87.0
      - run: cargo test --all

  test-msrv:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.87.0
      - run: cargo build --all
      - run: cargo test --all --no-run
      # No se corren los integration tests en MSRV
      # (pueden fallar por timeout en runners lentos)

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-llvm-cov
      - run: cargo llvm-cov --all --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v3
        with:
          files: lcov.info
```

### 8.2 Badges en README

```markdown
![CI](https://github.com/usuario/baud/workflows/CI/badge.svg)
![Coverage](https://codecov.io/gh/usuario/baud/branch/main/graph/badge.svg)
```

## 9. Cobertura Objetivo

### 9.1 Por Modulo

Ver RNF-06 en `docs/specs/requisitos.md`.

### 9.2 Como Medir

```bash
# Con cargo-llvm-cov
cargo install cargo-llvm-cov
cargo llvm-cov --all --html --output-dir coverage/
# Abrir coverage/index.html en navegador
```

### 9.3 Como Interpretar

- Coverage de domain (grid, parser): objetivo >60%
  en MVP, >80% en produccion.
- Coverage de infrastructure (pty): objetivo >40%
  en MVP, >65% en produccion.
- Coverage global: objetivo >50% en MVP, >60% en
  produccion.
- Coverage no es el único indicador: una cobertura
  del 100% con tests triviales no significa nada.
  Se priorizan tests que cubran edge cases reales.

## 10. Limitaciones

1. **vttest interactivo.** La automatizacion completa
   es dificil. La estrategia es ejecutar
   manualmente en milestones + complementar con
   esctest.
2. **criterion en CI es ruidoso.** Los runners
   compartidos tienen varianza alta. Se usa
   `cargo-criterion` con threshold del 15%.
3. **proptest no cubre todas las combinaciones.**
   Solo invariantes especificas del grid.
4. **Coverage tooling no captura branches
   perfectamente.** `cargo-llvm-cov` es el mejor
   disponible, pero tiene falsos negativos.
5. **Integration tests dependen de bash instalado.**
   Si el runner no tiene bash, fallan. Se asume
   que `ubuntu-latest` siempre tiene bash.

## 11. Referencias

- docs/decisions/ADR-0006-testing-strategy.md
  (decision de alto nivel).
- docs/prompts/iter-06-investigacion-D.md
  (investigacion base, 827 lineas).
- https://crates.io/crates/proptest
- https://crates.io/crates/criterion
- https://invisible-island.net/vttest/
- https://github.com/MarcusJohnson91/esctest
- https://github.com/alacritty/alacritty/tree/master/.github/workflows

## Cambios

| Version | Fecha      | Cambios |
|:--------|:-----------|:--------|
| 0.1.0   | 2026-06-14 | Primer borrador. Detalle operativo de la estrategia de 4 niveles. |
