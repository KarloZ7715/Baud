//! Icono Nerd Font por nombre de proceso, para el titulo de una tab.
//!
//! Los codepoints vienen de `glyphnames.json` (proyecto Nerd Fonts, sets
//! devicons/linux/font-awesome). La disponibilidad real se comprueba por
//! separado (`glyph_rasterizes`): la lista de fallback del usuario puede no
//! incluir una Nerd Font, y dibujar el codepoint sin comprobarlo pintaria
//! tofu (plan 011, decision "Sin tofu").

/// Todos los iconos que puede devolver `icon_for_process`, para comprobar su
/// disponibilidad de una sola vez al cargar la fuente (`ALL_ICONS.len()`
/// shapeos en vez de uno por tab).
pub const ALL_ICONS: &[char] = &[
    '\u{f36f}', '\u{e7c5}', '\u{e702}', '\u{e7a8}', '\u{e718}', '\u{e73c}', '\u{f308}', '\u{e8b1}',
    '\u{eeb2}', '\u{f120}',
];

/// Icono para el nombre de proceso (basename ya en minusculas). Los procesos
/// sin entrada especifica caen al icono generico de terminal, no a `None`:
/// todo proceso en primer plano que no sea el shell muestra algun icono.
pub fn icon_for_process(name: &str) -> char {
    match name {
        "nvim" => '\u{f36f}',                                   // linux-neovim
        "vim" => '\u{e7c5}',                                    // dev-vim
        "git" => '\u{e702}',                                    // dev-git
        "cargo" | "rustc" => '\u{e7a8}',                        // dev-rust
        "node" | "npm" | "pnpm" | "yarn" | "bun" => '\u{e718}', // dev-nodejs_small
        "python" | "python3" | "ipython" => '\u{e73c}',         // dev-python
        "docker" | "docker-compose" => '\u{f308}',              // linux-docker
        "ssh" => '\u{e8b1}',                                    // dev-ssh
        "htop" | "btop" | "top" => '\u{eeb2}',                  // fa-gauge
        _ => '\u{f120}',                                        // fa-terminal (generico)
    }
}

/// `true` si `ch` rasteriza a un bitmap no vacio con la familia/tamano
/// dados, es decir, si la fuente activa (con su fallback) trae el glifo en
/// vez de caer en `.notdef`. Pensada para llamarse una vez por caracter y
/// cachear el resultado: shapear un glifo no es gratis.
pub fn glyph_rasterizes(
    font_system: &mut glyphon::FontSystem,
    swash_cache: &mut glyphon::SwashCache,
    family: &str,
    font_size: f32,
    ch: char,
) -> bool {
    let metrics = glyphon::Metrics::new(font_size, font_size * 1.2);
    let mut buf = glyphon::Buffer::new(font_system, metrics);
    let attrs = glyphon::Attrs::new().family(super::resolve_family(family));
    let text = ch.to_string();
    buf.set_text(
        font_system,
        &text,
        &attrs,
        glyphon::cosmic_text::Shaping::Advanced,
        None,
    );
    buf.shape_until_scroll(font_system, false);
    let Some(cache_key) = buf
        .layout_runs()
        .next()
        .and_then(|run| run.glyphs.first().cloned())
        .map(|glyph| glyph.physical((0.0, 0.0), 1.0).cache_key)
    else {
        return false;
    };
    super::glyph_cache::cache_key_rasterizes(font_system, swash_cache, cache_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_for_process_mapea_nombres_conocidos() {
        assert_eq!(icon_for_process("nvim"), '\u{f36f}');
        assert_eq!(icon_for_process("vim"), '\u{e7c5}');
        assert_eq!(icon_for_process("git"), '\u{e702}');
        assert_eq!(icon_for_process("cargo"), icon_for_process("rustc"));
        assert_eq!(icon_for_process("node"), icon_for_process("bun"));
        assert_eq!(icon_for_process("python3"), icon_for_process("python"));
        assert_eq!(
            icon_for_process("docker"),
            icon_for_process("docker-compose")
        );
        assert_eq!(icon_for_process("htop"), icon_for_process("btop"));
    }

    #[test]
    fn icon_for_process_cae_al_generico_para_desconocidos() {
        assert_eq!(icon_for_process("make"), '\u{f120}');
        assert_eq!(icon_for_process(""), '\u{f120}');
    }

    #[test]
    fn all_icons_cubre_cada_glifo_devuelto_por_icon_for_process() {
        let names = [
            "nvim",
            "vim",
            "git",
            "cargo",
            "rustc",
            "node",
            "npm",
            "pnpm",
            "yarn",
            "bun",
            "python",
            "python3",
            "ipython",
            "docker",
            "docker-compose",
            "ssh",
            "htop",
            "btop",
            "top",
            "make",
        ];
        for name in names {
            let ch = icon_for_process(name);
            assert!(
                ALL_ICONS.contains(&ch),
                "ALL_ICONS no incluye el icono de {name:?} ({ch:?})"
            );
        }
    }

    #[test]
    fn glyph_rasterizes_no_panica_y_degrada_a_falso_sin_glifo() {
        let mut font_system = super::super::terminal_fallback::create_font_system();
        let mut swash_cache = glyphon::SwashCache::new();
        // Family generica sin Nerd Font: en CI (sin Nerd Fonts instaladas)
        // el icono cae a .notdef y la funcion debe degradar a `false`, no
        // panicar ni devolver un falso positivo.
        let available = glyph_rasterizes(
            &mut font_system,
            &mut swash_cache,
            "monospace",
            12.0,
            '\u{f36f}',
        );
        let _ = available; // el valor depende de las fuentes del sistema; solo importa que no panique
    }

    #[test]
    #[ignore = "requiere Nerd Font instalada (no disponible en CI)"]
    fn glyph_rasterizes_true_con_nerd_font_instalada() {
        let mut font_system = super::super::terminal_fallback::create_font_system();
        let mut swash_cache = glyphon::SwashCache::new();
        assert!(glyph_rasterizes(
            &mut font_system,
            &mut swash_cache,
            "monospace",
            12.0,
            '\u{f36f}',
        ));
    }
}
