//! El parser y el grid sobreviven a cualquier input.

use baud::ansi::Term;

#[test]
fn stress_random_bytes_and_resizes() {
    let mut term = Term::new();
    let mut parser = vte::Parser::new();
    let mut estado: u64 = 0x2026_0812;
    let mut chunk = [0u8; 4096];
    for i in 0..2560 {
        for b in chunk.iter_mut() {
            estado ^= estado << 13;
            estado ^= estado >> 7;
            estado ^= estado << 17;
            *b = (estado & 0xff) as u8;
        }
        parser.advance(&mut term, &chunk);
        if i % 64 == 0 {
            let rows = 5 + (estado % 196) as usize;
            let cols = 10 + ((estado >> 8) % 491) as usize;
            term.resize_grid(rows, cols, true);
        }
    }
    let rows = term.grid.rows_count;
    let cols = term.grid.cols_count;
    assert_eq!(term.grid.rows.len(), rows);
    assert_eq!(term.cursor.rows_count, rows);
    assert_eq!(term.cursor.cols_count, cols);
    assert!(term.cursor.row < rows && term.cursor.col < cols);
}
