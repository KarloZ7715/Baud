// Politica cero-crashes: unwrap/expect en runtime critico es lint
// (ansi, grid, pty, renderer, window, event_loop, session, input).
// Los invariantes demostrables llevan #[allow] con su justificacion.
pub mod ansi;
pub mod base64;
pub mod cli;
pub mod clipboard;
pub mod color;
pub mod color_scheme;
pub mod config;
#[cfg(windows)]
pub mod console;
pub mod copy_mode;
pub mod cursor;
pub mod diagnostics;
pub mod display_quirks;
pub mod event_loop;
pub mod grapheme;
pub mod grid;
pub mod input;
pub mod installation;
pub mod layout;
pub mod paste_overlay;
pub mod pty;
pub mod remote;
pub mod renderer;
pub mod search;
pub mod search_overlay;
pub mod selection;
pub mod session;
pub mod smart_select;
pub mod theme_picker;
pub mod updater;
pub mod watchdog;
pub mod window;
