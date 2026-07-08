//! Layout y etiquetas de la barra de tabs (ancho igual, scroll al foco).

/// Ancho minimo de una tab en celdas monoespaciadas.
pub const MIN_TAB_WIDTH_CELLS: usize = 10;
/// Ancho maximo ideal por tab cuando hay pocas.
pub const MAX_TAB_WIDTH_CELLS: usize = 36;

#[derive(Debug, Clone)]
pub struct TabSegment {
    pub index: usize,
    pub x_px: f32,
    pub width_px: f32,
    pub width_cells: usize,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct TabBarLayout {
    pub segments: Vec<TabSegment>,
    /// Linea monoespaciada lista para el buffer de glyphon.
    pub line: String,
    pub scroll_offset: usize,
    pub tab_width_cells: usize,
}

/// Acorta titulos OSC largos (prompts completos) a un nombre legible.
pub fn shorten_tab_title(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }

    if let Some(tail) = s.rsplit('/').next().filter(|t| !t.is_empty()) {
        let clean = tail.trim_end_matches(':');
        if clean.len() <= 24 && clean.len() < s.len() {
            return clean.to_string();
        }
    }

    if let Some(last) = s.split_whitespace().last() {
        let clean = last.trim_matches(|c: char| c == ':' || c == '│' || c == '▓');
        if !clean.is_empty() && clean.len() <= 20 && clean.len() * 2 < s.len() {
            return clean.to_string();
        }
    }

    truncate_chars(s, 16)
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

/// Centra o trunca `text` a exactamente `width` columnas monoespaciadas.
pub fn fit_to_cell_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len > width {
        if width == 1 {
            return "…".to_string();
        }
        let keep = width - 1;
        let head = keep / 2;
        let tail = keep - head;
        let mut out: String = chars.iter().take(head).collect();
        out.push('…');
        out.extend(chars.iter().skip(len.saturating_sub(tail)));
        return out;
    }
    let pad = width - len;
    let left = pad / 2;
    let right = pad - left;
    format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
}

fn format_tab_label(index: usize, title: &str, width_cells: usize) -> String {
    let inner = if title.is_empty() {
        format!("{index}")
    } else {
        title.to_string()
    };
    fit_to_cell_width(&inner, width_cells)
}

/// Distribuye tabs a ancho igual; hace scroll para mantener visible la activa.
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
            line: String::new(),
            scroll_offset: 0,
            tab_width_cells: 0,
        };
    }

    let available_cells = (bar_width_px / cell_w).floor() as usize;
    let available_cells = available_cells.max(1);

    let mut tab_cells = (available_cells / n).clamp(MIN_TAB_WIDTH_CELLS, MAX_TAB_WIDTH_CELLS);
    if tab_cells * n > available_cells {
        tab_cells = (available_cells / n).max(MIN_TAB_WIDTH_CELLS);
    }

    let mut visible = (available_cells / tab_cells).min(n).max(1);
    if tab_cells * visible > available_cells {
        tab_cells = (available_cells / visible).max(MIN_TAB_WIDTH_CELLS);
        visible = (available_cells / tab_cells).min(n).max(1);
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

    let mut segments = Vec::with_capacity(visible);
    let mut line = String::with_capacity(visible * tab_cells);
    let mut x = pad_x;

    for (i, title) in titles.iter().enumerate().skip(scroll).take(visible) {
        let short = shorten_tab_title(title);
        let label = format_tab_label(i + 1, &short, tab_cells);
        let width_px = tab_cells as f32 * cell_w;
        segments.push(TabSegment {
            index: i,
            x_px: x,
            width_px,
            width_cells: tab_cells,
            label: label.clone(),
            active: i == focused,
        });
        line.push_str(&label);
        x += width_px;
    }

    TabBarLayout {
        segments,
        line,
        scroll_offset: scroll,
        tab_width_cells: tab_cells,
    }
}

pub fn tab_index_at(
    layout: &TabBarLayout,
    x: f64,
    y: f64,
    pad_y: f32,
    cell_h: f32,
) -> Option<usize> {
    let top = f64::from(pad_y);
    let bottom = top + f64::from(cell_h);
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
    fn shorten_vacio_devuelve_vacio() {
        assert_eq!(shorten_tab_title(""), "");
    }

    #[test]
    fn fit_to_cell_width_centra_texto_corto() {
        assert_eq!(fit_to_cell_width("bash", 10), "   bash   ");
    }

    #[test]
    fn fit_to_cell_width_trunca_largo() {
        let s = fit_to_cell_width("abcdefghijklmnop", 8);
        assert_eq!(s.chars().count(), 8);
        assert!(s.contains('…'));
    }

    #[test]
    fn layout_reparte_ancho_igual() {
        let titles = vec!["one".into(), "two".into(), "three".into()];
        let layout = compute_layout(&titles, 0, 0.0, 300.0, 10.0);
        assert_eq!(layout.segments.len(), 3);
        let w0 = layout.segments[0].width_cells;
        assert!(layout.segments.iter().all(|s| s.width_cells == w0));
        assert_eq!(layout.line.chars().count(), w0 * 3);
    }

    #[test]
    fn layout_hace_scroll_para_tab_enfocada() {
        let titles: Vec<String> = (0..12).map(|i| format!("tab{i}")).collect();
        let layout = compute_layout(&titles, 11, 0.0, 200.0, 10.0);
        assert!(layout.scroll_offset > 0);
        assert!(layout.segments.iter().any(|s| s.index == 11 && s.active));
    }

    #[test]
    fn tab_index_at_resuelve_segmento() {
        let titles = vec!["a".into(), "b".into()];
        let layout = compute_layout(&titles, 0, 8.0, 200.0, 10.0);
        let seg0 = &layout.segments[0];
        let mid = (seg0.x_px + seg0.width_px * 0.5) as f64;
        assert_eq!(tab_index_at(&layout, mid, 6.0, 6.0, 20.0), Some(0));
    }
}
