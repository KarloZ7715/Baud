#![no_main]
// El parser tiene que digerir cualquier flujo de bytes sin panic.
// Es el contrato mínimo: un proceso hijo no puede tumbar el emulador.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut term = baud::ansi::Term::new();
    term.feed(data);
});
