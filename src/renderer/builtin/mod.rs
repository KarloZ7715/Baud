//! Render programatico de box-drawing, block elements y separadores Powerline.

mod block_elements;
mod box_drawing;
mod cache;
mod powerline;
mod stroke;

pub use cache::MaskCache;

/// Rango Unicode cubierto por builtins de caja/bloque.
pub const BUILTIN_START: u32 = 0x2500;
pub const BUILTIN_END: u32 = 0x259F;

/// Separadores Powerline (geometria de celda; no iconos E0A0..E0A3).
pub const POWERLINE_SEP_START: u32 = 0xE0B0;
pub const POWERLINE_SEP_END: u32 = 0xE0B3;

static MASK_CACHE: std::sync::OnceLock<MaskCache> = std::sync::OnceLock::new();

fn cache() -> &'static MaskCache {
    MASK_CACHE.get_or_init(MaskCache::new)
}

/// True si el caracter pertenece a box-drawing, block elements o separadores Powerline.
pub fn is_builtin_glyph(ch: char) -> bool {
    matches!(
        ch as u32,
        BUILTIN_START..=BUILTIN_END | POWERLINE_SEP_START..=POWERLINE_SEP_END
    )
}

/// True si el codepoint es geometria decorativa (no debe recibir boost de contraste).
pub fn is_geometric_glyph(ch: char) -> bool {
    is_builtin_glyph(ch)
}

/// True si el builtin puede rasterizar el caracter.
pub fn supports(ch: char) -> bool {
    if !is_builtin_glyph(ch) {
        return false;
    }
    box_drawing::supports_box(ch)
        || block_elements::supports_block(ch)
        || powerline::supports_powerline(ch)
}

/// Invalida cache (resize / cambio de fuente).
pub fn clear_cache() {
    cache().clear();
}

/// Mascara alpha para un builtin char. Usa cache por (ch, w, h).
pub fn render(ch: char, w: u32, h: u32) -> Option<Vec<u8>> {
    let wu = w as usize;
    let hu = h as usize;
    cache()
        .get_or_insert(ch, w, h, &mut || render_uncached(ch, wu, hu))
        .map(|arc| arc.to_vec())
}

/// Sin cache (tests y raster directo).
pub fn render_uncached(ch: char, w: usize, h: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    if box_drawing::is_box_drawing(ch) {
        return box_drawing::render_box(ch, w, h);
    }
    if block_elements::supports_block(ch) {
        return block_elements::render_block(ch, w, h);
    }
    if powerline::supports_powerline(ch) {
        return powerline::render_powerline(ch, w, h);
    }
    None
}

// Compat API publica
pub fn is_box_glyph(ch: char) -> bool {
    is_builtin_glyph(ch)
}

pub fn is_box_mask_supported(ch: char) -> bool {
    supports(ch)
}

pub fn box_mask(ch: char, w: usize, h: usize) -> Option<Vec<u8>> {
    render_uncached(ch, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_rangos_box_y_block() {
        assert!(is_builtin_glyph('\u{2500}'));
        assert!(is_builtin_glyph('\u{2588}'));
        assert!(is_builtin_glyph('\u{E0B0}'));
        assert!(!is_builtin_glyph('\u{E0A0}'));
        assert!(!is_builtin_glyph('A'));
        assert!(supports('\u{E0B2}'));
        assert!(!supports('\u{E0A0}'));
    }

    #[test]
    fn geometric_incluye_box_block_y_separadores_powerline() {
        assert!(is_geometric_glyph('\u{2500}'));
        assert!(is_geometric_glyph('\u{2580}'));
        assert!(is_geometric_glyph('\u{E0B0}'));
        assert!(is_geometric_glyph('\u{E0B3}'));
        assert!(!is_geometric_glyph('\u{E0A0}'));
        assert!(!is_geometric_glyph('A'));
    }

    #[test]
    fn render_cached_reuses_mask() {
        clear_cache();
        let a = render('\u{2500}', 10, 20).expect("a");
        let b = render('\u{2500}', 10, 20).expect("b");
        assert_eq!(a, b);
        assert!(cache().len() >= 1);
    }

    #[test]
    fn vertical_continuity_three_cells() {
        let w = 10u32;
        let h = 20u32;
        let midx = (w / 2) as usize;
        for _ in 0..3 {
            let m = render('\u{2502}', w, h).expect("v");
            assert!(m[midx] > 0);
            assert!(m[((h as usize) - 1) * w as usize + midx] > 0);
        }
    }
}
