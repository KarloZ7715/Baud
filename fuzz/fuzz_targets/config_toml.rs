#![no_main]
// El hot-reload parsea TOML a Config; un archivo corrupto no puede panic.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = toml::from_str::<baud::config::Config>(s);
    }
});
