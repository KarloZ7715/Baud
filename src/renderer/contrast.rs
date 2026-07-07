//! Ajuste perceptual de contraste fg/bg (OKLab + WCAG).

use std::collections::HashMap;

use oklab::{oklab_to_srgb, srgb_to_oklab, Oklab, Rgb};

use crate::color::contrast_ratio_rgb;

/// Cache LRU simple por frame: `(fg, bg, min_bits) -> adjusted fg`.
#[derive(Debug, Default)]
pub struct ContrastCache {
    entries: HashMap<(u32, u32, u64), (u8, u8, u8)>,
}

impl ContrastCache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn adjust(&mut self, fg: (u8, u8, u8), bg: (u8, u8, u8), min_ratio: f64) -> (u8, u8, u8) {
        let min_bits = min_ratio.to_bits();
        let key = (pack_rgb(fg), pack_rgb(bg), min_bits);
        if let Some(cached) = self.entries.get(&key) {
            return *cached;
        }
        let adjusted = adjust_fg(fg, bg, min_ratio);
        self.entries.insert(key, adjusted);
        adjusted
    }
}

fn pack_rgb(rgb: (u8, u8, u8)) -> u32 {
    u32::from(rgb.0) << 16 | u32::from(rgb.1) << 8 | u32::from(rgb.2)
}

/// Ajusta el foreground en OKLab hasta alcanzar `min_ratio` WCAG sobre `bg`.
pub fn adjust_fg(fg: (u8, u8, u8), bg: (u8, u8, u8), min_ratio: f64) -> (u8, u8, u8) {
    if min_ratio <= 1.0 || fg == bg {
        return fg;
    }
    if contrast_ratio_rgb(fg, bg) >= min_ratio {
        return fg;
    }

    let bg_lab = srgb_to_oklab(Rgb {
        r: bg.0,
        g: bg.1,
        b: bg.2,
    });
    let fg_lab = srgb_to_oklab(Rgb {
        r: fg.0,
        g: fg.1,
        b: fg.2,
    });

    let light_bg = bg_lab.l > 0.6;
    let (mut lo, mut hi) = if light_bg {
        (0.0f32, fg_lab.l)
    } else {
        (fg_lab.l, 1.0f32)
    };

    let mut best = fg;
    let mut best_ratio = contrast_ratio_rgb(fg, bg);

    for _ in 0..32 {
        let mid_l = (lo + hi) / 2.0;
        let candidate = oklab_to_srgb(Oklab {
            l: mid_l,
            a: fg_lab.a,
            b: fg_lab.b,
        });
        let rgb = (candidate.r, candidate.g, candidate.b);
        let ratio = contrast_ratio_rgb(rgb, bg);
        if ratio >= min_ratio {
            return rgb;
        }
        if ratio > best_ratio {
            best_ratio = ratio;
            best = rgb;
        }
        if light_bg {
            hi = mid_l;
        } else {
            lo = mid_l;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fg_eq_bg_passthrough() {
        let fg = (100, 50, 200);
        assert_eq!(adjust_fg(fg, fg, 3.0), fg);
    }

    #[test]
    fn min_ratio_one_passthrough() {
        let fg = (50, 50, 50);
        let bg = (60, 60, 60);
        assert_eq!(adjust_fg(fg, bg, 1.0), fg);
    }

    #[test]
    fn cobalt2_blue_on_blue_improves() {
        // Cobalt2: blue #0087ff on similar blue bg
        let fg = (0x00, 0x87, 0xff);
        let bg = (0x00, 0x5f, 0xd7);
        let before = contrast_ratio_rgb(fg, bg);
        let after = adjust_fg(fg, bg, 3.0);
        assert!(contrast_ratio_rgb(after, bg) >= 3.0);
        assert!(contrast_ratio_rgb(after, bg) > before);
    }

    #[test]
    fn solarized_comment_on_bg() {
        let fg = (0x58, 0x6e, 0x75);
        let bg = (0x00, 0x2b, 0x36);
        let adjusted = adjust_fg(fg, bg, 3.0);
        assert!(contrast_ratio_rgb(adjusted, bg) >= 3.0);
    }

    #[test]
    fn cache_hit_returns_same() {
        let mut cache = ContrastCache::default();
        let fg = (0x58, 0x6e, 0x75);
        let bg = (0x00, 0x2b, 0x36);
        let a = cache.adjust(fg, bg, 3.0);
        let b = cache.adjust(fg, bg, 3.0);
        assert_eq!(a, b);
        assert_eq!(cache.entries.len(), 1);
    }
}
