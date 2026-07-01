//! Render programatico de box-drawing y block elements.

mod block_elements;
mod box_drawing;
mod cache;
mod stroke;

pub use cache::MaskCache;

/// Rango Unicode cubierto por builtins.
pub const BUILTIN_START: u32 = 0x2500;
pub const BUILTIN_END: u32 = 0x259F;

static MASK_CACHE: std::sync::OnceLock<MaskCache> = std::sync::OnceLock::new();

fn cache() -> &'static MaskCache {
    MASK_CACHE.get_or_init(MaskCache::new)
}

/// True si el caracter pertenece a box-drawing o block elements.
pub fn is_builtin_glyph(ch: char) -> bool {
    matches!(ch as u32, BUILTIN_START..=BUILTIN_END)
}

/// True si el builtin puede rasterizar el caracter.
pub fn supports(ch: char) -> bool {
    if !is_builtin_glyph(ch) {
        return false;
    }
    box_drawing::supports_box(ch) || block_elements::supports_block(ch)
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
        assert!(!is_builtin_glyph('A'));
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
