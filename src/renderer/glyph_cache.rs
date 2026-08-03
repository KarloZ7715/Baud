//! Cache de glifos shaped por clave (sin GPU).

use std::collections::HashMap;

use glyphon::cosmic_text::FontSystem;
use glyphon::{ContentType, SwashCache, SwashContent};

use super::glyph::{shape_glyph, GlyphKey, GlyphStrings, LineYCache, ShapedGlyph, ShapedOverlay};
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
    /// `GlyphKey` -> id denso de texto.
    entries: HashMap<GlyphKey, u16>,
    /// Glifos indexados por `id - FIRST_TEXT_ID`. `None` representa un slot
    /// que fue invalidado sin recompactar (p. ej. `ensure_metrics` conserva
    /// `next_id` para no reutilizar ids que la GPU ya vio).
    glyphs: Vec<Option<CachedGlyph>>,
    next_id: u16,
    /// Métricas con las que se shapearon las entradas actuales.
    metrics_key: Option<MetricsCacheKey>,
    /// Cache de `reference_line_y` por `(family, bold, italic, dim)`,
    /// compartida por todos los glifos de celda (ver `shape_glyph`).
    line_y_cache: LineYCache,
    /// True tras purgar el cache por saturacion de `next_id` (ver
    /// `take_needs_atlas_reset`): el llamante debe resetear el atlas de la
    /// GPU, igual que hace tras `metrics_changed`.
    needs_atlas_reset: bool,
    /// LUT de la curva de contraste aplicada a las mascaras al rasterizar.
    mask_lut: [u8; 256],
    /// Clave (bits de `text_contrast`, polaridad oscuro/claro) con la que se
    /// construyo `mask_lut`. Con contraste 0.0 la polaridad no aplica y la
    /// clave es fija, porque la LUT es identidad.
    mask_curve_key: (u32, bool),
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
    /// Ids 0-10 reservados para mascaras de decoracion y chrome; texto empieza en 11.
    const FIRST_TEXT_ID: u16 = 11;

    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            glyphs: Vec::new(),
            next_id: Self::FIRST_TEXT_ID,
            metrics_key: None,
            line_y_cache: LineYCache::new(),
            needs_atlas_reset: false,
            mask_lut: build_mask_curve_lut(0.0),
            mask_curve_key: (0.0f32.to_bits(), true),
        }
    }

    /// Invalida entradas si las métricas cambiaron. Devuelve `true` si hay que resetear el atlas GPU.
    pub fn metrics_changed(&mut self, metrics: &CellMetrics) -> bool {
        let key = metrics_key_from(metrics);
        if self.metrics_key == Some(key) {
            return false;
        }
        self.purge_glyph_slots();
        self.metrics_key = Some(key);
        true
    }

    /// Purga entradas y slots dejando intacta la curva de contraste; el
    /// llamante decide si actualiza `metrics_key` o marca `needs_atlas_reset`.
    fn purge_glyph_slots(&mut self) {
        self.clear_entries();
        self.glyphs.clear();
        self.next_id = Self::FIRST_TEXT_ID;
    }

    /// Reconstruye la LUT de la curva de contraste y purga el cache si la
    /// curva cambio. Devuelve `true` si hay que resetear el atlas GPU, igual
    /// que [`GlyphCache::metrics_changed`]: las mascaras ya subidas al atlas
    /// llevan la curva anterior.
    ///
    /// `dark_theme` es la polaridad del tema (fondo mas oscuro que el texto):
    /// en temas oscuros la curva engorda el trazo y en claros lo adelgaza.
    /// Con `text_contrast == 0.0` la LUT es identidad y la polaridad no
    /// invalida, para no repoblar el atlas en cada cambio de tema.
    pub fn mask_curve_changed(&mut self, text_contrast: f32, dark_theme: bool) -> bool {
        let c = text_contrast.clamp(0.0, 1.0);
        let key = if c == 0.0 {
            (0.0f32.to_bits(), true)
        } else {
            (c.to_bits(), dark_theme)
        };
        if self.mask_curve_key == key {
            return false;
        }
        self.mask_curve_key = key;
        let k = if dark_theme { c } else { -c };
        self.mask_lut = build_mask_curve_lut(k);
        self.purge_glyph_slots();
        true
    }

    /// La LUT actual si la curva esta activa (contraste distinto de 0.0).
    fn active_mask_lut(&self) -> Option<&[u8; 256]> {
        (self.mask_curve_key.0 != 0.0f32.to_bits()).then_some(&self.mask_lut)
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
        self.glyphs.fill(None);
        self.line_y_cache.clear();
    }

    /// Devuelve el siguiente `custom_glyph_id`. Si `next_id` esta saturado
    /// (65535 glifos distintos vistos desde el ultimo reset de metricas),
    /// purga el cache entero en vez de seguir asignando el mismo id a
    /// glifos distintos: la alternativa (lo que hacia `saturating_add` antes
    /// de este cambio) deja `by_id` apuntando siempre a la ultima entrada
    /// insertada y todo glifo nuevo se pinta como esa.
    fn take_next_id(&mut self) -> u16 {
        if self.next_id == u16::MAX {
            tracing::warn!(
                "glyph cache: next_id saturado en {} entradas, purgando cache de glifos",
                u16::MAX
            );
            self.purge_glyph_slots();
            self.needs_atlas_reset = true;
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// True (una sola vez) si una purga por saturacion de `next_id` requiere
    /// resetear el atlas de la GPU. El llamante debe consultarlo tras cada
    /// frame que pudo insertar glifos nuevos.
    pub fn take_needs_atlas_reset(&mut self) -> bool {
        std::mem::take(&mut self.needs_atlas_reset)
    }

    fn insert_glyph(&mut self, key: GlyphKey, cached: CachedGlyph) -> u16 {
        let id = cached.custom_glyph_id;
        let idx = (id - Self::FIRST_TEXT_ID) as usize;
        if idx >= self.glyphs.len() {
            self.glyphs.resize_with(idx + 1, || None);
        }
        self.glyphs[idx] = Some(cached);
        self.entries.insert(key, id);
        id
    }

    /// Devuelve el id del glifo cacheado o lo shapea, rasteriza e inserta.
    ///
    /// Devuelve el id (no `&CachedGlyph`) para que el llamador pueda leer los
    /// datos con [`GlyphCache::get_by_custom_id`] y [`GlyphCache::overlays`]
    /// sin mantener un prestamo mientras inserta overlays en el mismo cache.
    pub fn get_or_insert(
        &mut self,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: &CellMetrics,
        strings: &GlyphStrings,
        key: GlyphKey,
    ) -> u16 {
        self.ensure_metrics(metrics);

        if let Some(&id) = self.entries.get(&key) {
            return id;
        }

        let shaped = shape_glyph(
            font_system,
            metrics,
            &key,
            strings.family(key.family),
            strings.extra(key.extra),
            &mut self.line_y_cache,
        );
        let raster = rasterize_shaped(font_system, swash_cache, &shaped, self.active_mask_lut());
        let custom_glyph_id = self.take_next_id();
        self.insert_glyph(
            key,
            CachedGlyph {
                custom_glyph_id,
                shaped,
                raster,
            },
        )
    }

    /// Inserta o devuelve un glifo ya shaped (p. ej. de un run con ligaduras).
    /// `shaped` solo se clona si la entrada no existia.
    pub fn get_or_insert_shaped(
        &mut self,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: &CellMetrics,
        key: GlyphKey,
        shaped: &ShapedGlyph,
    ) -> u16 {
        self.ensure_metrics(metrics);

        if let Some(&id) = self.entries.get(&key) {
            return id;
        }

        let raster = rasterize_shaped(font_system, swash_cache, shaped, self.active_mask_lut());
        let custom_glyph_id = self.take_next_id();
        self.insert_glyph(
            key,
            CachedGlyph {
                custom_glyph_id,
                shaped: shaped.clone(),
                raster,
            },
        )
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Invalida entradas tras cambio de metricas de celda (resize).
    pub fn clear(&mut self) {
        self.purge_glyph_slots();
        self.metrics_key = None;
    }

    pub fn get(&self, key: &GlyphKey) -> Option<&CachedGlyph> {
        self.entries
            .get(key)
            .and_then(|&id| self.get_by_custom_id(id))
    }

    pub fn get_by_custom_id(&self, id: u16) -> Option<&CachedGlyph> {
        if id < Self::FIRST_TEXT_ID {
            return None;
        }
        self.glyphs
            .get((id - Self::FIRST_TEXT_ID) as usize)
            .and_then(|slot| slot.as_ref())
    }

    /// Capas overlay del glifo con ese id (vacio si no tiene o no existe).
    pub fn overlays(&self, id: u16) -> &[ShapedOverlay] {
        self.get_by_custom_id(id)
            .map(|c| c.shaped.overlays.as_slice())
            .unwrap_or(&[])
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

/// LUT de 256 entradas con la curva de contraste de la mascara:
/// `a' = round(255 * (a/255)^(1/(1+k)))`. `k > 0` sube los medios (trazo mas
/// grueso, para temas oscuros), `k < 0` los baja (temas claros) y `k = 0` es
/// la identidad. Se aplica una vez por glifo al rasterizar, nunca por frame.
fn build_mask_curve_lut(k: f32) -> [u8; 256] {
    // El divisor acotado evita el polo de k = -1 (contraste maximo en tema
    // claro): el resultado es una curva muy pronunciada, no un NaN.
    let exponent = 1.0 / (1.0 + k).max(0.01);
    let mut lut = [0u8; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        *entry = (255.0 * (i as f32 / 255.0).powf(exponent)).round() as u8;
    }
    lut
}

fn rasterize_shaped(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    shaped: &ShapedGlyph,
    mask_curve: Option<&[u8; 256]>,
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

    let (mut data, width, height) = normalize_raster_bytes(
        &image.data,
        image.placement.width,
        image.placement.height,
        content_type,
    );

    // La curva de contraste solo toca mascaras de un canal; los bitmaps de
    // color (emoji) se dejan tal cual.
    if let (Some(lut), ContentType::Mask) = (mask_curve, content_type) {
        for byte in data.iter_mut() {
            *byte = lut[*byte as usize];
        }
    }

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

    fn test_key(strings: &mut GlyphStrings, ch: char, family: &str) -> GlyphKey {
        GlyphKey {
            ch,
            extra: 0,
            bold: false,
            italic: false,
            dim: false,
            lig_slot: None,
            family: strings.intern_family(family),
        }
    }

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
            1.0,
        );
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let key = test_key(&mut strings, 'X', &font_config.family);

        let first_id =
            cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
        assert_eq!(cache.len(), 1);
        assert_eq!(first_id, GlyphCache::FIRST_TEXT_ID);

        let second_id =
            cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
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
            1.0,
        );
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let prompt = "baud master ● ? ❯ hola";

        for ch in prompt.chars() {
            if ch == ' ' {
                continue;
            }
            let key = test_key(&mut strings, ch, &font_config.family);
            let id =
                cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
            let cached = cache.get_by_custom_id(id).expect("glifo cacheado");
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
    fn purga_al_saturar_next_id_y_senala_reset_de_atlas() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let font_config = FontConfig::default();
        let metrics = CellMetrics::measure(
            &mut font_system,
            &font_config.family,
            font_config.size as f32,
            font_config.line_height,
            font_config.glyph_offset,
            1.0,
        );
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();

        let key_a = test_key(&mut strings, 'A', &font_config.family);
        cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics,
            &strings,
            key_a,
        );
        assert!(!cache.take_needs_atlas_reset());

        // Forzar la saturacion sin insertar 65 mil glifos reales.
        cache.next_id = u16::MAX;
        let key_b = test_key(&mut strings, 'B', &font_config.family);
        let id_b = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics,
            &strings,
            key_b,
        );

        assert_eq!(
            id_b,
            GlyphCache::FIRST_TEXT_ID,
            "tras la purga, el primer id vuelve a ser el inicial"
        );
        assert_eq!(
            cache.len(),
            1,
            "la purga debe vaciar las entradas viejas, solo queda la nueva"
        );
        assert!(
            cache.take_needs_atlas_reset(),
            "una purga por saturacion debe pedir reset de atlas"
        );
        assert!(
            !cache.take_needs_atlas_reset(),
            "la señal se consume una sola vez"
        );
    }

    #[test]
    fn cache_se_invalida_al_cambiar_metricas() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let family = FontConfig::default().family;
        use crate::config::GlyphOffset;
        let offset = GlyphOffset { x: 0.0, y: 0.0 };
        let metrics_12 = CellMetrics::measure(&mut font_system, &family, 12.0, 1.0, offset, 1.0);
        let metrics_14 = CellMetrics::measure(&mut font_system, &family, 14.0, 1.3, offset, 1.0);
        let mut strings = GlyphStrings::new();
        let key = test_key(&mut strings, 'M', &family);
        let mut cache = GlyphCache::new();
        assert!(cache.metrics_changed(&metrics_12));
        let id_a = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics_12,
            &strings,
            key,
        );
        let h_12 = cache.get_by_custom_id(id_a).expect("glifo").raster.height;
        assert!(cache.metrics_changed(&metrics_14));
        let id_b = cache.get_or_insert(
            &mut font_system,
            &mut swash_cache,
            &metrics_14,
            &strings,
            key,
        );
        let b = cache.get_by_custom_id(id_b).expect("glifo");
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
    fn lut_curva_identidad_sin_contraste() {
        let lut = build_mask_curve_lut(0.0);
        for (i, &v) in lut.iter().enumerate() {
            assert_eq!(v, i as u8);
        }
    }

    #[test]
    fn lut_curva_tema_oscuro_engorda_y_claro_adelgaza() {
        let oscura = build_mask_curve_lut(0.5);
        assert_eq!(oscura[0], 0);
        assert_eq!(oscura[255], 255);
        assert!(
            oscura[128] > 128,
            "tema oscuro: la curva debe subir los medios (engordar)"
        );

        let clara = build_mask_curve_lut(-0.5);
        assert_eq!(clara[0], 0);
        assert_eq!(clara[255], 255);
        assert!(
            clara[128] < 128,
            "tema claro: la curva debe bajar los medios (adelgazar)"
        );
    }

    #[test]
    fn mask_curve_changed_purga_solo_si_la_curva_cambia() {
        let mut cache = GlyphCache::new();
        // Curva por defecto (0.0): cualquier polaridad es la misma identidad.
        assert!(!cache.mask_curve_changed(0.0, true));
        assert!(!cache.mask_curve_changed(0.0, false));
        // Cambio de contraste: purga y pide reset de atlas.
        assert!(cache.mask_curve_changed(0.5, true));
        assert!(!cache.mask_curve_changed(0.5, true));
        // Con contraste activo la polaridad si forma parte de la curva.
        assert!(cache.mask_curve_changed(0.5, false));
        assert!(!cache.mask_curve_changed(0.5, false));
        // Volver a 0.0 tambien invalida.
        assert!(cache.mask_curve_changed(0.0, true));
    }

    #[test]
    fn curva_se_aplica_solo_a_mascaras_al_rasterizar() {
        let mut font_system = create_font_system();
        let mut swash_cache = SwashCache::new();
        let family = FontConfig::default().family;
        let metrics = CellMetrics::measure(
            &mut font_system,
            &family,
            12.0,
            1.0,
            crate::config::GlyphOffset { x: 0.0, y: 0.0 },
            1.0,
        );
        let mut strings = GlyphStrings::new();
        let key = test_key(&mut strings, 'M', &family);

        let mut cache = GlyphCache::new();
        let id_plano =
            cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
        let plano = cache
            .get_by_custom_id(id_plano)
            .expect("glifo")
            .raster
            .clone();
        assert_eq!(plano.content_type, ContentType::Mask);

        assert!(cache.mask_curve_changed(0.8, true));
        let id_curvo =
            cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
        let curvo = &cache.get_by_custom_id(id_curvo).expect("glifo").raster;

        assert_eq!(plano.data.len(), curvo.data.len());
        assert_ne!(
            plano.data, curvo.data,
            "la curva debe cambiar los bytes de la mascara"
        );
        // Cada byte curvo es el byte plano pasado por la LUT del tema oscuro.
        let lut = build_mask_curve_lut(0.8);
        for (&a, &b) in plano.data.iter().zip(curvo.data.iter()) {
            assert_eq!(b, lut[a as usize]);
        }
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
            1.0,
        );
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        for ch in ['😀', '中'] {
            let key = test_key(&mut strings, ch, &font_config.family);
            let id =
                cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
            let cached = cache.get_by_custom_id(id).expect("glifo cacheado");
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
            1.0,
        );
        let mut cache = GlyphCache::new();
        let mut strings = GlyphStrings::new();
        let chars = ['┌', '─', '│', '┐', '\u{e0b0}', '\u{f0239}'];

        for ch in chars {
            let key = test_key(&mut strings, ch, &font_config.family);
            let id =
                cache.get_or_insert(&mut font_system, &mut swash_cache, &metrics, &strings, key);
            let cached = cache.get_by_custom_id(id).expect("glifo cacheado");
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
