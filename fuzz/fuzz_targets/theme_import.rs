#![no_main]
// Un archivo de tema externo puede ser cualquiera de los cuatro formatos
// detectados por nombre. Se fuerzan los cuatro para no depender del sondeo.
use libfuzzer_sys::fuzz_target;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        for name in ["foot.ini", "ghostty.conf", "kitty.conf", "alacritty.toml"] {
            let _ = baud::config::theme_import::import_from_str(s, Path::new(name));
        }
    }
});
