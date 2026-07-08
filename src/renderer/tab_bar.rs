//! Layout y chrome de la barra de tabs.
//!
//! Modelo inspirado en Ghostty: una fila compacta, tabs alineadas al padding de la
//! ventana, tab activa con el mismo fondo que el terminal, separador fino y un
//! pequeño hueco antes del grid.

/// Filas del grid reservadas para la barra de tabs.
pub const TAB_BAR_HEIGHT_ROWS: usize = 1;
/// Hueco entre la barra y la primera fila del terminal (px).
pub const TAB_CONTENT_GAP_PX: f32 = 3.0;
/// Separacion entre tabs en columnas monoespaciadas.
pub const TAB_GAP_CELLS: usize = 1;
/// Ancho minimo por tab (muchas tabs / scroll).
pub const MIN_TAB_WIDTH_CELLS: usize = 8;
/// Ancho maximo por tab.
pub const MAX_TAB_WIDTH_CELLS: usize = 24;
/// Columnas reservadas para indicador de scroll (‹ / ›).
pub const SCROLL_INDICATOR_CELLS: usize = 2;
/// Columnas reservadas para el boton cerrar (×) al hacer hover.
pub const TAB_CLOSE_WIDTH_CELLS: usize = 1;
/// Padding horizontal interno del titulo (1 celda a cada lado).
pub const TAB_LABEL_PAD_CELLS: usize = 1;

#[derive(Debug, Clone, Copy, Default)]
pub struct TabBarMouseState {
    pub hover_index: Option<usize>,
    /// Tab que muestra el boton × (persiste durante fade-out).
    pub close_tab: Option<usize>,
    /// Opacidad animada del boton cerrar (0..1).
    pub close_alpha: f32,
}

#[derive(Debug, Clone)]
pub struct TabSegment {
    pub index: usize,
    /// Origen X en pixeles (esquina superior izquierda del tab).
    pub x_px: f32,
    pub width_px: f32,
    pub width_cells: usize,
    /// Titulo acortado para etiqueta.
    pub title_short: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct TabBarLayout {
    pub segments: Vec<TabSegment>,
    pub scroll_offset: usize,
    pub tab_width_cells: usize,
    pub show_scroll_left: bool,
    pub show_scroll_right: bool,
    pub mouse: TabBarMouseState,
}

/// Altura de la barra en pixeles.
#[inline]
pub fn tab_bar_height_px(cell_h: f32) -> f32 {
    cell_h * TAB_BAR_HEIGHT_ROWS as f32
}

/// Espacio vertical total reservado (barra + hueco antes del grid).
#[inline]
pub fn tab_chrome_reserve_px(cell_h: f32) -> f32 {
    tab_bar_height_px(cell_h) + TAB_CONTENT_GAP_PX
}

/// Ancho util de la barra con padding horizontal simetrico.
#[inline]
pub fn tab_bar_inner_width(surface_w: f32, pad_x: f32) -> f32 {
    (surface_w - pad_x * 2.0).max(0.0)
}

/// Acorta titulos OSC largos (prompts completos) a un nombre legible.
pub fn shorten_tab_title(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }

    if let Some(tail) = s.rsplit('/').next().filter(|t| !t.is_empty()) {
        let clean = tail.trim_end_matches(':');
        if !clean.is_empty() && clean.len() <= 24 && clean.len() < s.len() {
            return clean.to_string();
        }
    }

    if let Some(last) = s.split_whitespace().last() {
        let clean = last.trim_matches(|c: char| c == ':' || c == '│' || c == '▓');
        if !clean.is_empty() && clean.len() <= 20 && clean.len() * 2 < s.len() {
            return clean.to_string();
        }
    }

    truncate_chars(s, 12)
}

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Trunca por el final.
pub fn truncate_end(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out: String = chars.iter().take(width - 1).collect();
    out.push('…');
    out
}

/// Etiqueta visible dentro de `width_cells` columnas (alineada a la izquierda).
pub fn format_tab_label(index: usize, title: &str, width_cells: usize) -> String {
    if width_cells == 0 {
        return String::new();
    }
    if width_cells <= 3 || title.is_empty() {
        let idx = format!("{index}");
        return truncate_end(&idx, width_cells);
    }
    truncate_end(title, width_cells)
}

fn cell_budget(tab_cells: usize, count: usize, gap: usize) -> usize {
    count * tab_cells + gap * count.saturating_sub(1)
}

fn variable_cell_budget(widths: &[usize], gap: usize) -> usize {
    widths.iter().sum::<usize>() + gap * widths.len().saturating_sub(1)
}

/// Ancho en columnas segun el titulo (padding + hueco para ×).
pub fn tab_content_cells(index_1based: usize, title: &str) -> usize {
    let short = shorten_tab_title(title);
    let text_len = if short.is_empty() {
        index_1based.to_string().chars().count()
    } else {
        short.chars().count()
    };
    let need = text_len + TAB_LABEL_PAD_CELLS * 2 + TAB_CLOSE_WIDTH_CELLS;
    need.clamp(MIN_TAB_WIDTH_CELLS, MAX_TAB_WIDTH_CELLS)
}

#[expect(
    clippy::too_many_arguments,
    reason = "layout builder mirrors compute_layout inputs"
)]
fn build_segments(
    titles: &[String],
    focused: usize,
    cell_w: f32,
    gap: usize,
    widths: &[usize],
    scroll: usize,
    visible: usize,
    x_start: f32,
) -> Vec<TabSegment> {
    let mut x = x_start;
    let mut segments = Vec::with_capacity(visible);
    for (i, title) in titles.iter().enumerate().skip(scroll).take(visible) {
        let w = widths[i];
        let width_px = w as f32 * cell_w;
        segments.push(TabSegment {
            index: i,
            x_px: x,
            width_px,
            width_cells: w,
            title_short: shorten_tab_title(title),
            active: i == focused,
        });
        x += (w + gap) as f32 * cell_w;
    }
    segments
}

/// Distribuye tabs; hace scroll para mantener visible la activa.
pub fn compute_layout(
    titles: &[String],
    focused: usize,
    pad_x: f32,
    bar_width_px: f32,
    cell_w: f32,
) -> TabBarLayout {
    let n = titles.len();
    if n == 0 || cell_w <= 0.0 {
        return TabBarLayout {
            segments: Vec::new(),
            scroll_offset: 0,
            tab_width_cells: 0,
            show_scroll_left: false,
            show_scroll_right: false,
            mouse: TabBarMouseState::default(),
        };
    }

    let gap = TAB_GAP_CELLS;
    let available = (bar_width_px / cell_w).floor() as usize;
    let available = available.max(1);

    let content_widths: Vec<usize> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| tab_content_cells(i + 1, t))
        .collect();

    if variable_cell_budget(&content_widths, gap) <= available {
        let segments = build_segments(titles, focused, cell_w, gap, &content_widths, 0, n, pad_x);
        let max_w = segments.iter().map(|s| s.width_cells).max().unwrap_or(0);
        return TabBarLayout {
            segments,
            scroll_offset: 0,
            tab_width_cells: max_w,
            show_scroll_left: false,
            show_scroll_right: false,
            mouse: TabBarMouseState::default(),
        };
    }

    let max_fit = |tab_cells: usize| -> usize {
        if tab_cells == 0 {
            return 1;
        }
        ((available + gap) / (tab_cells + gap)).max(1)
    };

    let mut tab_cells = if n <= max_fit(MIN_TAB_WIDTH_CELLS) {
        let per = (available.saturating_sub(gap * n.saturating_sub(1))) / n;
        per.clamp(MIN_TAB_WIDTH_CELLS, MAX_TAB_WIDTH_CELLS)
    } else {
        MIN_TAB_WIDTH_CELLS
    };
    tab_cells = tab_cells.max(1);

    let mut visible = n.min(max_fit(tab_cells));
    while visible > 1 && cell_budget(tab_cells, visible, gap) > available {
        visible -= 1;
    }
    visible = visible.max(1);

    let needs_scroll = visible < n;
    let ind_left = usize::from(needs_scroll);
    let ind_right = usize::from(needs_scroll);
    let ind_cells = (ind_left + ind_right) * SCROLL_INDICATOR_CELLS;
    let tabs_available = available.saturating_sub(ind_cells);

    while visible > 1 && cell_budget(tab_cells, visible, gap) > tabs_available {
        visible -= 1;
    }

    if visible < n {
        let per = (tabs_available.saturating_sub(gap * visible.saturating_sub(1))) / visible;
        tab_cells = per.clamp(MIN_TAB_WIDTH_CELLS, MAX_TAB_WIDTH_CELLS).max(1);
        while visible > 1 && cell_budget(tab_cells, visible, gap) > tabs_available {
            visible -= 1;
        }
    }

    let mut scroll = 0usize;
    if visible < n {
        if focused >= scroll + visible {
            scroll = focused + 1 - visible;
        }
        if focused < scroll {
            scroll = focused;
        }
        scroll = scroll.min(n.saturating_sub(visible));
    }

    let show_scroll_left = needs_scroll && scroll > 0;
    let show_scroll_right = needs_scroll && scroll + visible < n;

    let uniform_widths: Vec<usize> = vec![tab_cells; n];
    let x_start = pad_x + (ind_left * SCROLL_INDICATOR_CELLS) as f32 * cell_w;
    let segments = build_segments(
        titles,
        focused,
        cell_w,
        gap,
        &uniform_widths,
        scroll,
        visible,
        x_start,
    );

    TabBarLayout {
        segments,
        scroll_offset: scroll,
        tab_width_cells: tab_cells,
        show_scroll_left,
        show_scroll_right,
        mouse: TabBarMouseState::default(),
    }
}

/// Etiqueta del titulo; deja columnas libres a la derecha si `reserve_close`.
pub fn segment_title_label(
    index: usize,
    title: &str,
    width_cells: usize,
    reserve_close: bool,
) -> String {
    let inner = width_cells.saturating_sub(TAB_LABEL_PAD_CELLS * 2);
    if inner == 0 {
        return String::new();
    }
    if !reserve_close || inner <= TAB_CLOSE_WIDTH_CELLS {
        return format_tab_label(index, title, inner);
    }
    let text_w = inner.saturating_sub(TAB_CLOSE_WIDTH_CELLS);
    format_tab_label(index, title, text_w)
}

/// Posicion X del boton cerrar (ultima columna del tab).
#[inline]
pub fn segment_close_left_px(seg: &TabSegment, cell_w: f32) -> f32 {
    seg.x_px + seg.width_px - cell_w * TAB_CLOSE_WIDTH_CELLS as f32
}

/// Fondo solido detras del × (coordenadas relativas a la celda del boton).
pub fn push_close_scrub(
    bar_h: f32,
    cell_w: f32,
    alpha: f32,
    active: bool,
    theme: &crate::config::ThemeConfig,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    use glyphon::CustomGlyph;

    use crate::renderer::decorations::SOLID_MASK_GLYPH_ID;

    if alpha <= 0.02 {
        return;
    }
    let (br, bg, bb) = if active {
        crate::config::parse_hex(&theme.background)
    } else {
        crate::config::parse_hex(&theme.black)
    };
    let base_a = if active { 255.0 } else { 200.0 };
    let a = (base_a * alpha.clamp(0.0, 1.0)) as u8;
    let scrub_w = cell_w * TAB_CLOSE_WIDTH_CELLS as f32;
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: 0.0,
        width: scrub_w,
        height: bar_h,
        color: Some(glyphon::Color::rgba(br, bg, bb, a)),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

/// Resaltado sutil al hacer hover en una tab inactiva.
pub fn build_inactive_hover_chrome(
    width_px: f32,
    bar_h: f32,
    alpha: f32,
    theme: &crate::config::ThemeConfig,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    use glyphon::CustomGlyph;

    use crate::renderer::decorations::SOLID_MASK_GLYPH_ID;

    let (br, bg, bb) = crate::config::parse_hex(&theme.black);
    let a = (112.0 * alpha.clamp(0.0, 1.0)) as u8;
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: 0.0,
        width: width_px,
        height: bar_h,
        color: Some(glyphon::Color::rgba(br, bg, bb, a)),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

pub fn tab_close_at(
    layout: &TabBarLayout,
    x: f64,
    y: f64,
    pad_y: f32,
    bar_h: f32,
    cell_w: f32,
) -> Option<usize> {
    if layout.mouse.close_alpha < 0.35 {
        return None;
    }
    let hover = layout.mouse.hover_index?;
    let top = f64::from(pad_y);
    let bottom = top + f64::from(bar_h);
    if y < top || y >= bottom {
        return None;
    }
    let xf = x as f32;
    for seg in &layout.segments {
        if seg.index == hover {
            let close_left = seg.x_px + seg.width_px - cell_w;
            if (close_left..seg.x_px + seg.width_px).contains(&xf) {
                return Some(seg.index);
            }
        }
    }
    None
}

pub fn tab_index_at(
    layout: &TabBarLayout,
    x: f64,
    y: f64,
    pad_y: f32,
    bar_h: f32,
) -> Option<usize> {
    let top = f64::from(pad_y);
    let bottom = top + f64::from(bar_h);
    if y < top || y >= bottom {
        return None;
    }
    let xf = x as f32;
    for seg in &layout.segments {
        if xf >= seg.x_px && xf < seg.x_px + seg.width_px {
            return Some(seg.index);
        }
    }
    None
}

/// Chrome de la zona de tabs: pista sutil + separador inferior.
pub fn build_tab_track(
    inner_w: f32,
    bar_h: f32,
    theme: &crate::config::ThemeConfig,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    use glyphon::CustomGlyph;

    use crate::renderer::decorations::SOLID_MASK_GLYPH_ID;

    out.clear();

    let (br, bg, bb) = crate::config::parse_hex(&theme.black);
    let chrome = glyphon::Color::rgba(br, bg, bb, 48);
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: 0.0,
        width: inner_w.max(0.0),
        height: bar_h,
        color: Some(chrome),
        snap_to_physical_pixel: true,
        metadata: 0,
    });

    let (fr, fg, fb) = crate::config::parse_hex(&theme.foreground);
    let rule = glyphon::Color::rgba(fr, fg, fb, 36);
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: bar_h - 1.0,
        width: inner_w.max(0.0),
        height: 1.0,
        color: Some(rule),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

/// Chrome de un segmento (coordenadas relativas al TextArea del tab).
pub fn build_segment_chrome(
    width_px: f32,
    bar_h: f32,
    active: bool,
    theme: &crate::config::ThemeConfig,
    out: &mut Vec<glyphon::CustomGlyph>,
) {
    use glyphon::CustomGlyph;

    use crate::renderer::decorations::SOLID_MASK_GLYPH_ID;

    out.clear();
    if !active {
        return;
    }
    let (br, bg, bb) = crate::config::parse_hex(&theme.background);
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: 0.0,
        width: width_px,
        height: bar_h,
        color: Some(glyphon::Color::rgb(br, bg, bb)),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
    let (cr, cg, cb) = crate::config::parse_hex(&theme.cursor);
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: bar_h - 2.0,
        width: width_px,
        height: 2.0,
        color: Some(glyphon::Color::rgb(cr, cg, cb)),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_extrae_basename_de_ruta() {
        assert_eq!(
            shorten_tab_title("carloscc@cachy-ccc ~/Documentos/Dev/baud"),
            "baud"
        );
    }

    #[test]
    fn layout_tabs_cortas_se_ajustan_al_titulo() {
        let titles = vec!["baud".into(), "baud".into(), "baud".into()];
        let layout = compute_layout(&titles, 0, 0.0, 1200.0, 10.0);
        assert_eq!(layout.segments.len(), 3);
        assert!(layout
            .segments
            .iter()
            .all(|s| s.width_cells < MAX_TAB_WIDTH_CELLS));
        let total: f32 = layout
            .segments
            .iter()
            .map(|s| s.width_px + TAB_GAP_CELLS as f32 * 10.0)
            .sum();
        assert!(total < 400.0);
    }

    #[test]
    fn layout_reparte_ancho_igual_con_pocas_tabs() {
        let titles = vec!["one".into(), "two".into(), "three".into()];
        let layout = compute_layout(&titles, 0, 0.0, 600.0, 10.0);
        assert_eq!(layout.segments.len(), 3);
        assert!(layout
            .segments
            .iter()
            .all(|s| s.width_cells >= MIN_TAB_WIDTH_CELLS));
    }

    #[test]
    fn layout_ancho_proporcional_a_titulo_largo() {
        let titles = vec!["ab".into(), "abcdefghijklmnop".into()];
        let layout = compute_layout(&titles, 0, 0.0, 800.0, 10.0);
        assert!(layout.segments[1].width_cells > layout.segments[0].width_cells);
    }

    #[test]
    fn layout_estrecha_y_oculta_tabs_con_muchas() {
        let titles: Vec<String> = (0..20).map(|i| format!("tab{i}")).collect();
        let layout = compute_layout(&titles, 0, 0.0, 400.0, 10.0);
        assert!(layout.tab_width_cells >= MIN_TAB_WIDTH_CELLS);
        assert!(layout.segments.len() < 20);
        assert!(layout.show_scroll_left || layout.show_scroll_right);
    }

    #[test]
    fn layout_hace_scroll_para_tab_enfocada() {
        let titles: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let layout = compute_layout(&titles, 11, 0.0, 200.0, 10.0);
        assert!(layout.scroll_offset > 0);
        assert!(layout.segments.iter().any(|s| s.index == 11 && s.active));
    }

    #[test]
    fn layout_limita_ancho_con_pocas_tabs() {
        let titles = vec!["one".into(), "two".into()];
        let layout = compute_layout(&titles, 0, 0.0, 1200.0, 10.0);
        assert!(layout
            .segments
            .iter()
            .all(|s| s.width_cells <= MAX_TAB_WIDTH_CELLS));
        let span: f32 = layout.segments.last().unwrap().x_px
            + layout.segments.last().unwrap().width_px
            - layout.segments[0].x_px;
        assert!(span < 300.0);
    }

    #[test]
    fn segmentos_alineados_en_celdas() {
        let titles = vec!["a".into(), "b".into()];
        let cell_w = 10.0;
        let layout = compute_layout(&titles, 0, 8.0, 400.0, cell_w);
        let s0 = &layout.segments[0];
        let s1 = &layout.segments[1];
        let expected_stride = (s0.width_cells + TAB_GAP_CELLS) as f32 * cell_w;
        assert!((s1.x_px - s0.x_px - expected_stride).abs() < 0.01);
    }

    #[test]
    fn tab_index_at_resuelve_segmento() {
        let titles = vec!["a".into(), "b".into()];
        let layout = compute_layout(&titles, 0, 8.0, 400.0, 10.0);
        let seg0 = &layout.segments[0];
        let mid = (seg0.x_px + seg0.width_px * 0.5) as f64;
        let bar_h = tab_bar_height_px(20.0);
        assert_eq!(tab_index_at(&layout, mid, 10.0, 6.0, bar_h), Some(0));
    }

    #[test]
    fn tab_chrome_reserve_incluye_hueco() {
        assert_eq!(tab_chrome_reserve_px(20.0), 23.0);
    }

    #[test]
    fn tab_bar_inner_width_es_simetrico() {
        assert_eq!(tab_bar_inner_width(800.0, 8.0), 784.0);
    }

    #[test]
    fn segment_title_reserva_columna_cerrar() {
        let label = segment_title_label(1, "baud", 10, true);
        assert_eq!(label, "baud");
        assert!(label.chars().count() <= 9);
    }

    #[test]
    fn segment_close_left_en_ultima_columna() {
        let titles = vec!["baud".into()];
        let layout = compute_layout(&titles, 0, 0.0, 80.0, 10.0);
        let seg = &layout.segments[0];
        assert_eq!(
            segment_close_left_px(seg, 10.0),
            seg.x_px + seg.width_px - 10.0
        );
    }

    #[test]
    fn tab_close_at_detecta_zona_derecha() {
        let titles = vec!["a".into(), "b".into()];
        let mut layout = compute_layout(&titles, 0, 0.0, 400.0, 10.0);
        layout.mouse.hover_index = Some(0);
        layout.mouse.close_alpha = 1.0;
        let seg = &layout.segments[0];
        let x = (seg.x_px + seg.width_px - 5.0) as f64;
        assert_eq!(tab_close_at(&layout, x, 10.0, 0.0, 20.0, 10.0), Some(0));
    }

    #[test]
    fn tab_content_cells_incluye_padding_y_cerrar() {
        assert_eq!(tab_content_cells(1, "baud"), MIN_TAB_WIDTH_CELLS);
    }

    #[test]
    fn etiqueta_estrecha_usa_indice() {
        let label = format_tab_label(3, "baud", 3);
        assert_eq!(label, "3");
    }
}
