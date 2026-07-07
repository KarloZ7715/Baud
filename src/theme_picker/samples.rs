//! Bloques ANSI estáticos para el panel de muestras del theme picker.

use crate::ansi::Term;

/// Columnas del grid sintético de muestras.
pub const SAMPLE_COLS: usize = 56;
/// Filas reservadas para el bloque de muestras.
pub const SAMPLE_ROWS: usize = 48;

/// Muestra de código con colores 0–15 y sintaxis simulada.
pub fn code_sample() -> &'static [u8] {
    b"\
\x1b[38;5;8m// baud - sample de codigo\x1b[0m\n\
\x1b[38;5;2mfn\x1b[0m \x1b[38;5;6mprocess\x1b[0m(\x1b[38;5;3mdata\x1b[0m: &\x1b[38;5;6mstr\x1b[0m) \x1b[38;5;2m->\x1b[0m \x1b[38;5;6mResult\x1b[0m<()> {\n\
  \x1b[38;5;2mlet\x1b[0m \x1b[38;5;4mvalue\x1b[0m = \x1b[38;5;3m42\x1b[0m;\n\
  \x1b[38;5;2mif\x1b[0m \x1b[38;5;4mvalue\x1b[0m < \x1b[38;5;3m0\x1b[0m {\n\
    \x1b[38;5;1meprintln!\x1b[0m(\x1b[38;5;3m\"invalid input\"\x1b[0m);\n\
  }\n\
  \x1b[38;5;1mOk\x1b[0m(())\n\
}\n"
}

/// Prompt de shell decorado con salida de comando.
pub fn prompt_sample() -> &'static [u8] {
    b"\
\x1b[38;5;2muser\x1b[0m@\x1b[38;5;4mhost\x1b[0m \x1b[38;5;6m~/proj\x1b[0m \x1b[38;5;5mmain\x1b[0m \x1b[38;5;3m$\x1b[0m ls -la\n\
\x1b[38;5;8mdrwxr-xr-x\x1b[0m  \x1b[38;5;6msrc\x1b[0m/\n\
\x1b[38;5;8m-rw-r--r--\x1b[0m  \x1b[38;5;2mCargo.toml\x1b[0m\n\
\x1b[38;5;8m-rw-r--r--\x1b[0m  \x1b[38;5;4mREADME.md\x1b[0m\n\
\x1b[38;5;2muser\x1b[0m@\x1b[38;5;4mhost\x1b[0m \x1b[38;5;6m~/proj\x1b[0m \x1b[38;5;5mmain\x1b[0m \x1b[38;5;3m$\x1b[0m cargo build\n"
}

/// Salida tipo git status.
pub fn git_sample() -> &'static [u8] {
    b"\
\x1b[38;5;2mOn branch\x1b[0m \x1b[38;5;6mmain\x1b[0m\n\
\x1b[38;5;3mChanges not staged:\x1b[0m\n\
  \x1b[38;5;1mmodified:\x1b[0m   \x1b[38;5;4msrc/main.rs\x1b[0m\n\
  \x1b[38;5;1mmodified:\x1b[0m   \x1b[38;5;4msrc/config/mod.rs\x1b[0m\n\
\x1b[38;5;2mUntracked files:\x1b[0m\n\
  \x1b[38;5;3m??\x1b[0m \x1b[38;5;8mnotes.txt\x1b[0m\n"
}

/// Niveles de log (info / warn / error).
pub fn log_sample() -> &'static [u8] {
    b"\
\x1b[38;5;6m[INFO]\x1b[0m  server listening on :8080\n\
\x1b[38;5;3m[WARN]\x1b[0m  deprecated API: use v2 instead\n\
\x1b[38;5;1m[ERR]\x1b[0m   connection refused (127.0.0.1:5432)\n\
\x1b[38;5;2m[OK]\x1b[0m    config reloaded successfully\n"
}

/// Texto con bold, underline y colores basicos.
pub fn text_sample() -> &'static [u8] {
    b"\
\x1b[1mBold\x1b[0m \x1b[4munderline\x1b[0m \x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m \x1b[33myellow\x1b[0m \x1b[34mblue\x1b[0m \x1b[35mmagenta\x1b[0m \x1b[36mcyan\x1b[0m\n\
\x1b[38;5;8mcomentario tenue\x1b[0m | \x1b[38;5;7mtexto normal\x1b[0m | \x1b[1;38;5;15mtexto brillante\x1b[0m\n"
}

const SAMPLE_TITLES: [&str; 5] = [
    "── code ──\n",
    "── shell ──\n",
    "── git ──\n",
    "── log ──\n",
    "── text ──\n",
];

/// Todas las muestras concatenadas (para tests de parseo).
pub fn all_samples() -> Vec<&'static [u8]> {
    vec![
        code_sample(),
        prompt_sample(),
        git_sample(),
        log_sample(),
        text_sample(),
    ]
}

fn feed(term: &mut Term, data: &[u8]) {
    let mut parser = vte::Parser::new();
    parser.advance(term, data);
}

/// Construye un `Term` sintético con las muestras ANSI (mismo pipeline que el terminal).
pub fn build_sample_term() -> Term {
    let mut term = Term::new();
    term.resize_grid(SAMPLE_ROWS, SAMPLE_COLS, false);
    // LF debe volver al margen izquierdo (LNM); sin esto las filas quedan en
    // escalera porque el cursor conserva la columna al avanzar de línea.
    term.newline_mode = true;
    term.cursor_visible = false;
    for (title, sample) in SAMPLE_TITLES.iter().zip(all_samples()) {
        feed(&mut term, title.as_bytes());
        feed(&mut term, sample);
        feed(&mut term, b"\n");
    }
    term
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::Color;
    use crate::config::ThemeConfig;
    use crate::renderer::Palette;

    #[test]
    fn muestras_no_panic_vte() {
        let _term = build_sample_term();
    }

    #[test]
    fn colores_resuelven_via_palette() {
        let theme = ThemeConfig::default();
        let palette = Palette::from_theme(&theme);
        let mut term = Term::new();
        feed(&mut term, b"\x1b[31mR\x1b[0m");
        let cell = &term.active_grid().rows[0][0];
        assert_eq!(cell.attrs.fg, Color::Red);
        let (r, _, _) = palette.rgb(Color::Red, false);
        assert!(r > 0);
    }

    #[test]
    fn sample_term_tiene_contenido_clave() {
        let term = build_sample_term();
        let grid = term.active_grid();
        let flat: String = grid
            .rows
            .iter()
            .flat_map(|row| row.iter().map(|c| c.ch))
            .collect();
        assert!(flat.contains("process"));
        assert!(flat.contains("user@host"));
        assert!(flat.contains("config/mod.rs"));
        assert!(flat.contains("[ERR]"));
        assert!(flat.contains("if"));
    }

    #[test]
    fn picker_samples_palabras_intactas() {
        let term = build_sample_term();
        let flat: String = term
            .active_grid()
            .rows
            .iter()
            .flat_map(|row| row.iter().map(|c| c.ch))
            .collect();
        for word in ["if", "config/mod.rs", "[ERR]"] {
            assert!(flat.contains(word), "falta '{word}' en muestras");
        }
    }

    #[test]
    fn sample_term_filas_empiezan_en_margen() {
        let term = build_sample_term();
        let mut violations = Vec::new();
        for (ri, row) in term.active_grid().rows.iter().enumerate() {
            let Some(first_col) = row.iter().position(|c| c.ch != ' ' && c.ch != '\0') else {
                continue;
            };
            // Indentación intencional en código (2 espacios) o git (2 espacios).
            if first_col > 2 {
                let snippet: String = row.iter().skip(first_col).take(20).map(|c| c.ch).collect();
                violations.push(format!("row {ri} first_col={first_col} text={snippet:?}"));
            }
        }
        assert!(
            violations.is_empty(),
            "filas con margen anómalo (escalera):\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn terminal_y_picker_mismo_ajuste_contraste() {
        use crate::config::try_preset;
        use crate::renderer::{ContrastCache, Palette};

        let theme = try_preset("cobalt2").unwrap();
        let palette = Palette::from_theme(&theme);
        let term = build_sample_term();
        let mut cache = ContrastCache::default();

        for row in &term.active_grid().rows {
            for cell in row {
                if cell.ch == ' ' || cell.attrs.fg == crate::ansi::Color::Default {
                    continue;
                }
                let fg = palette.rgb(cell.attrs.fg, cell.attrs.bold);
                let bg = palette.bg_rgb(cell.attrs.bg);
                if fg == bg {
                    continue;
                }
                let adjusted = cache.adjust(fg, bg, theme.minimum_contrast);
                let mut cache2 = ContrastCache::default();
                assert_eq!(adjusted, cache2.adjust(fg, bg, theme.minimum_contrast));
                return;
            }
        }
        panic!("no se encontró celda coloreada en muestras");
    }
}
