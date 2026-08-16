#![no_main]
// Reflow adversarial: bytes y resizes intercalados. El parser se reutiliza
// para que las secuencias partidas entre chunks sigan siendo válidas.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let mut term = baud::ansi::Term::new();
    let mut parser = vte::Parser::new();
    for chunk in data.chunks(64) {
        parser.advance(&mut term, chunk);
        let rows = 1 + (chunk[0] as usize % 300);
        let cols = 1 + (chunk[chunk.len() - 1] as usize % 500);
        term.resize_grid(rows, cols, true);
    }
});
