//! Cache de glifos shaped por clave (sin GPU).

use std::collections::HashMap;

use glyphon::cosmic_text::FontSystem;
use glyphon::{ContentType, SwashCache, SwashContent};

use super::glyph::{shape_glyph, GlyphKey, ShapedGlyph};
use super::limits::{self, MAX_RASTER_BYTES};
use super::metrics::CellMetrics;

/// Bitmap rasterizado listo para el callback de `CustomGlyph`.
#[derive(Debug, Clone)]
pub struct CachedRaster {
    pub content_type: ContentType,
    pub data: Vec<u8>,
    pub width: u16,
    pub height: u16,
    /// Offset horizontal del bitmap respecto al origen del glifo (swash placement).
    pub placement_left: i32,
    /// Offset vertical del bitmap respecto al origen del glifo (swash placement).
    pub placement_top: i32,
    /// True si swash no pudo rasterizar el glifo (no pintar caja 1x1).
    pub missing: bool,
}

/// Entrada en cache con id para `CustomGlyph`.
#[derive(Debug, Clone)]
pub struct CachedGlyph {
    pub custom_glyph_id: u16,
    pub shaped: ShapedGlyph,
    pub raster: CachedRaster,
}

/// Cache en memoria de glifos shaped.
#[derive(Debug)]
pub struct GlyphCache {
    entries: HashMap<GlyphKey, CachedGlyph>,
    by_id: HashMap<u16, GlyphKey>,
    next_id: u16,
    /// Métricas con las que se shapearon las entradas actuales.
    metrics_key: Option<MetricsCacheKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetricsCacheKey {
    font_size_bits: u32,
    cell_w_bits: u32,
    cell_h_bits: u32,
}

fn metrics_key_from(metrics: &CellMetrics) -> MetricsCacheKey {
    MetricsCacheKey {
        font_size_bits: metrics.font_size.to_bits(),
        cell_w_bits: metrics.cell_w.to_bits(),
        cell_h_bits: metrics.cell_h.to_bits(),
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphCache {
    /// Ids 0-7 reservados para mascaras de decoracion; texto empieza en 8.
    const FIRST_TEXT_ID: u16 = 8;

    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            by_id: HashMap::new(),
            next_id: Self::FIRST_TEXT_ID,
            metrics_key: None,
        }
    }

    /// Invalida entradas si las métricas cambiaron. Devuelve `true` si hay que resetear el atlas GPU.
    pub fn metrics_changed(&mut self, metrics: &CellMetrics) -> bool {
        let key = metrics_key_from(metrics);
        if self.metrics_key == Some(key) {
            return false;
        }
        self.clear_entries();
        self.next_id = Self::FIRST_TEXT_ID;
        self.metrics_key = Some(key);
        true
    }

    fn ensure_metrics(&mut self, metrics: &CellMetrics) {
        let key = metrics_key_from(metrics);
        if self.metrics_key == Some(key) {
            return;
        }
        // Fallback sin atlas (p. ej. tests): conservar next_id para no reutilizar ids en GPU.
        self.clear_entries();
        self.metrics_key = Some(key);
    }

    fn clear_entries(&mut self) {
        self.entries.clear();
        self.by_id.clear();
    }

    /// Devuelve el glifo cacheado o lo shapea, rasteriza e inserta.
    pub fn get_or_insert(
        &mut self,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: &CellMetrics,
        family: &str,
        key: GlyphKey,
    ) -> &CachedGlyph {
        use std::collections::hash_map::Entry;

        self.ensure_metrics(metrics);

        match self.entries.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(vacant) => {
                let shaped = shape_glyph(font_system, metrics, vacant.key(), family);
                let raster = rasterize_shaped(font_system, swash_cache, &shaped);
                let custom_glyph_id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                self.by_id.insert(custom_glyph_id, vacant.key().clone());
                vacant.insert(CachedGlyph {
                    custom_glyph_id,
                    shaped,
                    raster,
                })
            }
        }
    }

    /// Inserta o devuelve un glifo ya shaped (p. ej. de un run con ligaduras).
    pub fn get_or_insert_shaped(
        &mut self,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: &CellMetrics,
        key: GlyphKey,
        shaped: ShapedGlyph,
    ) -> &CachedGlyph {
        use std::collections::hash_map::Entry;

        self.ensure_metrics(metrics);

        match self.entries.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(vacant) => {
                let raster = rasterize_shaped(font_system, swash_cache, &shaped);
                let custom_glyph_id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                self.by_id.insert(custom_glyph_id, vacant.key().clone());
                vacant.insert(CachedGlyph {
                    custom_glyph_id,
                    shaped,
                    raster,
                })
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Invalida entradas tras cambio de metricas de celda (resize).
    pub fn clear(&mut self) {
        self.clear_entries();
        self.next_id = Self::FIRST_TEXT_ID;
        self.metrics_key = None;
    }

    pub fn get(&self, key: &GlyphKey) -> Option<&CachedGlyph> {
        self.entries.get(key)
    }

    pub fn get_by_custom_id(&self, id: u16) -> Option<&CachedGlyph> {
        self.by_id.get(&id).and_then(|key| self.entries.get(key))
    }
}

/// True si swash puede rasterizar este cache key (bitmap no vacio).
pub(crate) fn cache_key_rasterizes(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cache_key: glyphon::CacheKey,
) -> bool {
    swash_cache
        .get_image_uncached(font_system, cache_key)
        .is_some_and(|image| !image.data.is_empty())
}

fn rasterize_shaped(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    shaped: &ShapedGlyph,
) -> CachedRaster {
    let Some(image) = swash_cache.get_image_uncached(font_system, shaped.cache_key) else {
        tracing::debug!("swash sin imagen para cache_key {:?}", shaped.cache_key);
        return missing_raster();
    };

    if image.data.is_empty() {
        return missing_raster();
    }

    if image.data.len() > MAX_RASTER_BYTES {
        tracing::warn!(
            "swash bitmap demasiado grande ({} bytes), omitiendo glifo",
            image.data.len()
        );
        return missing_raster();
    }

    let content_type = match image.content {
        SwashContent::Color => ContentType::Color,
        SwashContent::Mask | SwashContent::SubpixelMask => ContentType::Mask,
    };

    let (data, width, height) = normalize_raster_bytes(
        &image.data,
        image.placement.width,
        image.placement.height,
        content_type,
    );

    if width == 0 || height == 0 {
        return missing_raster();
    }

    CachedRaster {
        content_type,
        data,
        width,
        height,
        placement_left: image.placement.left,
        placement_top: image.placement.top,
        missing: false,
    }
}

const MAX_GLYPH_DIM: u32 = limits::MAX_GLYPH_DIM;

/// Garantiza dimensiones seguras y `data.len() == width * height * bpp`.
pub(crate) fn normalize_raster_bytes(
    data: &[u8],
    placement_w: u32,
    placement_h: u32,
    content_type: ContentType,
) -> (Vec<u8>, u16, u16) {
    let bpp = content_type.bytes_per_pixel();
    let (width, height) = derive_raster_dimensions(data.len(), bpp, placement_w, placement_h);
    let expected = width as usize * height as usize * bpp;

    if expected == 0 {
        return (vec![255], 1, 1);
    }

    if data.len() == expected {
        return (data.to_vec(), width, height);
    }

    if data.is_empty() {
        let fill = if content_type == ContentType::Mask {
            255
        } else {
            0
        };
        return (vec![fill; expected], width, height);
    }

    if data.len() > expected {
        return (data[..expected].to_vec(), width, height);
    }

    let mut out = data.to_vec();
    let pad = if content_type == ContentType::Mask {
        255
    } else {
        0
    };
    out.resize(expected, pad);
    (out, width, height)
}

fn derive_raster_dimensions(
    data_len: usize,
    bpp: usize,
    placement_w: u32,
    placement_h: u32,
) -> (u16, u16) {
    if data_len == 0 {
        return (1, 1);
    }
    let pixels = data_len / bpp.max(1);
    if pixels == 0 {
        return (1, 1);
    }

    let pw = placement_w.min(MAX_GLYPH_DIM);
    let ph = placement_h.min(MAX_GLYPH_DIM);
    if pw > 0 && ph > 0 && pw as usize * ph as usize == pixels {
        return (pw as u16, ph as u16);
    }

    if pw > 0 && pw <= MAX_GLYPH_DIM && pixels.is_multiple_of(pw as usize) {
        let h = pixels / pw as usize;
        if h <= MAX_GLYPH_DIM as usize {
            return (pw as u16, h as u16);
        }
    }

    let w = pixels.min(MAX_GLYPH_DIM as usize).max(1) as u16;
    (w, 1)
}

fn missing_raster() -> CachedRaster {
    CachedRaster {
        content_type: ContentType::Mask,
        data: Vec::new(),
        width: 0,
        height: 0,
        placement_left: 0,
        placement_top: 0,
        missing: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FontConfig;

    use super::super::terminal_fallback::create_font_system;

    #[test]
    fn cache_hit_on_second_lookup() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        );
        let mut cache = GlyphCache::new();
        let key = GlyphKey {
            ch: 'X',
            bold: false,
            italic: false,
            dim: false,
            family: font_config.family.clone(),
        };

        let first_id = cache
            .get_or_insert(
                &mut font_system,
                &mut swash_cache,
                &metrics,
                &font_config.family,
                key.clone(),
            )
            .custom_glyph_id;
        assert_eq!(cache.len(), 1);
        assert_eq!(first_id, 8);

        let second_id = cache
            .get_or_insert(
                &mut font_system,
                &mut swash_cache,
                &metrics,
                &font_config.family,
                key,
            )
            .custom_glyph_id;
        assert_eq!(first_id, second_id);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn normalize_raster_bytes_fills_empty_mask() {
        let (data, w, h) = normalize_raster_bytes(&[], 1, 1, ContentType::Mask);
        assert_eq!((w, h), (1, 1));
        assert_eq!(data, vec![255]);
    }

    #[test]
    fn normalize_raster_bytes_preserves_valid_mask() {
        let src = vec![128u8];
        let (data, w, h) = normalize_raster_bytes(&src, 1, 1, ContentType::Mask);
        assert_eq!((w, h), (1, 1));
        assert_eq!(data, src);
    }

    #[test]
    fn nerd_prompt_chars_rasterize_bounded() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        );
        let mut cache = GlyphCache::new();
        let prompt = "baud master ● ? ❯ hola";

        for ch in prompt.chars() {
            if ch == ' ' {
                continue;
            }
            let key = GlyphKey {
                ch,
                bold: false,
                italic: false,
                dim: false,
                family: font_config.family.clone(),
            };
            let cached = cache.get_or_insert(
                &mut font_system,
                &mut swash_cache,
                &metrics,
                &font_config.family,
                key,
            );
            assert!(
                !cached.raster.missing,
                "char {:?} debe rasterizar con fallback avanzado",
                ch
            );
            assert!(
                cached.raster.data.len() <= super::super::limits::MAX_RASTER_BYTES,
                "char {:?} raster {} bytes",
                ch,
                cached.raster.data.len()
            );
            assert!(
                cached.raster.width <= super::super::limits::MAX_GLYPH_DIM as u16,
                "char {:?} width {}",
                ch,
                cached.raster.width
            );
        }
    }

    #[test]
    fn cache_se_invalida_al_cambiar_metricas() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let family = FontConfig::default().family;
        use crate::config::GlyphOffset;
        let offset = GlyphOffset { x: 0.0, y: 0.0 };
        let metrics_12 = CellMetrics::measure(&mut font_system, &family, 12.0, 1.0, offset);
        let metrics_14 = CellMetrics::measure(&mut font_system, &family, 14.0, 1.3, offset);
        let key = GlyphKey {
            ch: 'M',
            bold: false,
            italic: false,
            dim: false,
            family: family.clone(),
        };
        let mut cache = GlyphCache::new();
        assert!(cache.metrics_changed(&metrics_12));
        let a = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics_12,
            &family,
            key.clone(),
        );
        let h_12 = a.raster.height;
        assert!(cache.metrics_changed(&metrics_14));
        let b = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics_14,
            &family,
            key,
        );
        assert_ne!(
            h_12, b.raster.height,
            "altura bitmap debe reflejar nuevo tamaño"
        );
        assert_eq!(
            cache.len(),
            1,
            "solo debe quedar la entrada del nuevo tamaño"
        );
    }

    #[test]
    #[ignore = "requiere fuente CJK (no disponible en CI)"]
    fn emoji_y_cjk_rasterizan_con_fallback() {
        let mut font_system = super::super::terminal_fallback::create_font_system();
        let mut swash_cache = SwashCache::new();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        );
        let mut cache = GlyphCache::new();
        for ch in ['😀', '中'] {
            let key = GlyphKey {
                ch,
                bold: false,
                italic: false,
                dim: false,
                family: font_config.family.clone(),
            };
            let cached = cache.get_or_insert(
                &mut font_system,
                &mut swash_cache,
                &metrics,
                &font_config.family,
                key,
            );
            assert!(
                !cached.raster.missing,
                "char {:?} debe rasterizar (fallback emoji/CJK)",
                ch
            );
            if ch == '😀' {
                assert_eq!(
                    cached.raster.content_type,
                    ContentType::Color,
                    "emoji debe rasterizar como bitmap a color"
                );
            }
        }
    }

    #[test]
    #[ignore = "requiere Nerd Font instalada (no disponible en CI); box_glyph activo usa box_mask, no GlyphCache"]
    fn box_drawing_and_nerd_icons_rasterize() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
        );
        let mut cache = GlyphCache::new();
        let chars = ['┌', '─', '│', '┐', '\u{e0b0}', '\u{f0239}'];

        for ch in chars {
            let key = GlyphKey {
                ch,
                bold: false,
                italic: false,
                dim: false,
                family: font_config.family.clone(),
            };
            let cached = cache.get_or_insert(
                &mut font_system,
                &mut swash_cache,
                &metrics,
                &font_config.family,
                key,
            );
            assert!(
                !cached.raster.missing,
                "char {ch:?} (U+{:04X}) no debe ser tofu",
                ch as u32
            );
            assert!(
                cached.raster.width > 1 || cached.raster.height > 1,
                "char {ch:?} no debe colapsar a pixel 1x1"
            );
        }
    }
}
