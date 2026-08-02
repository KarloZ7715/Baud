//! Barra de título propia para el modo `decorations = "custom"`.
//!
//! Proporciona layout, hit-testing y quads de fondo; los iconos de los botones
//! se renderizan como texto desde `renderer/mod.rs`.

use crate::config::{parse_hex, ThemeConfig};
use crate::renderer::decorations::{
    SOLID_MASK_GLYPH_ID, WIN_BTN_CLOSE_MASK_GLYPH_ID, WIN_BTN_MAXIMIZE_MASK_GLYPH_ID,
    WIN_BTN_MINIMIZE_MASK_GLYPH_ID, WIN_BTN_RESTORE_MASK_GLYPH_ID,
};
use glyphon::CustomGlyph;

/// Altura mínima de la barra en píxeles lógicos.
pub const TITLE_BAR_HEIGHT_LOGICAL: f32 = 34.0;
/// Ancho de cada botón de ventana en píxeles lógicos.
pub const TITLE_BUTTON_WIDTH_LOGICAL: f32 = 46.0;
/// Alto de cada botón de ventana en píxeles lógicos.
pub const TITLE_BUTTON_HEIGHT_LOGICAL: f32 = 32.0;

/// Tipos de botón de la barra de título.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleButtonKind {
    Minimize,
    Maximize,
    Close,
}

/// Botón de ventana dentro de la barra.
#[derive(Debug, Clone, Copy)]
pub struct TitleButton {
    pub kind: TitleButtonKind,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl TitleButton {
    /// Indica si el punto (px) cae dentro del botón.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        let xf = x as f32;
        let yf = y as f32;
        xf >= self.left
            && xf < self.left + self.width
            && yf >= self.top
            && yf < self.top + self.height
    }
}

/// Layout completo de la barra de título.
#[derive(Debug, Clone)]
pub struct TitleBarLayout {
    pub bar_height: f32,
    /// Origen X y ancho reservado para tabs o título.
    pub tab_area_x: f32,
    pub tab_area_width: f32,
    /// Botones de ventana, de izquierda a derecha.
    pub buttons: Vec<TitleButton>,
}

/// Resultado de hit-test sobre la barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarHit {
    /// Zona de arrastre de la ventana.
    Drag,
    /// Botón de ventana.
    Button(TitleButtonKind),
    /// Área reservada para tabs / título.
    TabArea,
}

/// Altura física de la barra: nunca menor que una fila de celda.
#[inline]
pub fn title_bar_height_px(cell_h: f32, scale_factor: f32) -> f32 {
    cell_h.max(TITLE_BAR_HEIGHT_LOGICAL * scale_factor)
}

/// Tamaño físico de un botón de ventana.
#[inline]
pub fn title_button_size_px(scale_factor: f32) -> (f32, f32) {
    (
        TITLE_BUTTON_WIDTH_LOGICAL * scale_factor,
        TITLE_BUTTON_HEIGHT_LOGICAL * scale_factor,
    )
}

/// Calcula el layout de la barra para el ancho de ventana dado.
pub fn compute_title_bar_layout(
    surface_w: f32,
    pad_x: f32,
    cell_h: f32,
    scale_factor: f32,
) -> TitleBarLayout {
    let bar_h = title_bar_height_px(cell_h, scale_factor);
    let (btn_w, btn_h) = title_button_size_px(scale_factor);
    let buttons_total_w = btn_w * 3.0;
    let buttons_x = (surface_w - pad_x - buttons_total_w).max(pad_x);
    let tab_area_x = pad_x;
    let tab_area_width = (buttons_x - pad_x - btn_w * 0.5).max(0.0);

    let top = (bar_h - btn_h) * 0.5;
    let buttons = vec![
        TitleButton {
            kind: TitleButtonKind::Minimize,
            left: buttons_x,
            top,
            width: btn_w,
            height: btn_h,
        },
        TitleButton {
            kind: TitleButtonKind::Maximize,
            left: buttons_x + btn_w,
            top,
            width: btn_w,
            height: btn_h,
        },
        TitleButton {
            kind: TitleButtonKind::Close,
            left: buttons_x + btn_w * 2.0,
            top,
            width: btn_w,
            height: btn_h,
        },
    ];

    TitleBarLayout {
        bar_height: bar_h,
        tab_area_x,
        tab_area_width,
        buttons,
    }
}

/// Resuelve qué región de la barra fue clickeada.
pub fn hit_test(layout: &TitleBarLayout, x: f64, y: f64, bar_top: f32) -> Option<TitleBarHit> {
    if y < f64::from(bar_top) || y >= f64::from(bar_top + layout.bar_height) {
        return None;
    }
    for btn in &layout.buttons {
        if btn.contains(x, y) {
            return Some(TitleBarHit::Button(btn.kind));
        }
    }
    let xf = x as f32;
    if xf >= layout.tab_area_x && xf < layout.tab_area_x + layout.tab_area_width {
        Some(TitleBarHit::TabArea)
    } else {
        Some(TitleBarHit::Drag)
    }
}

/// Dibuja la pista de fondo de la barra de título.
pub fn build_title_bar_track(
    surface_w: f32,
    bar_h: f32,
    theme: &ThemeConfig,
    out: &mut Vec<CustomGlyph>,
) {
    out.clear();
    let (br, bg, bb) = parse_hex(&theme.black);
    let track = glyphon::Color::rgba(br, bg, bb, 48);
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: 0.0,
        width: surface_w.max(0.0),
        height: bar_h,
        color: Some(track),
        snap_to_physical_pixel: true,
        metadata: 0,
    });

    let (fr, fg, fb) = parse_hex(&theme.foreground);
    let rule = glyphon::Color::rgba(fr, fg, fb, 36);
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: 0.0,
        top: bar_h - 1.0,
        width: surface_w.max(0.0),
        height: 1.0,
        color: Some(rule),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

/// Fondo sutil de un botón bajo hover.
/// `alpha` (0..1) escala la opacidad del fondo de hover: anima la entrada y
/// salida (~100 ms) en vez de un corte binario.
pub fn build_button_hover(
    btn: &TitleButton,
    active: bool,
    alpha: f32,
    theme: &ThemeConfig,
    out: &mut Vec<CustomGlyph>,
) {
    out.clear();
    if alpha <= 0.0 {
        return;
    }
    let a = alpha.clamp(0.0, 1.0);
    let color = if active {
        // Rojo de Fluent: se lee como "cerrar" en cualquier tema, claro u oscuro.
        glyphon::Color::rgba(0xc4, 0x2b, 0x1c, (255.0 * a).round() as u8)
    } else {
        let (fr, fg, fb) = parse_hex(&theme.foreground);
        glyphon::Color::rgba(fr, fg, fb, (26.0 * a).round() as u8)
    };
    out.push(CustomGlyph {
        id: SOLID_MASK_GLYPH_ID,
        left: btn.left,
        top: btn.top,
        width: btn.width,
        height: btn.height,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

/// Lado del icono de un boton de ventana en px logicos (cuadrado).
pub const WIN_BUTTON_ICON_SIZE_LOGICAL: f32 = 10.0;

/// Empuja la mascara vectorial del icono de un boton de ventana.
///
/// `maximized` alterna entre maximizar y restaurar para el boton `Maximize`.
/// El icono se centra dentro del boton y se dimensiona en px fisicos para
/// que el grosor del trazo sea entero a cualquier escala.
pub fn push_button_icon(
    btn: &TitleButton,
    kind: TitleButtonKind,
    maximized: bool,
    scale_factor: f32,
    color: glyphon::Color,
    out: &mut Vec<CustomGlyph>,
) {
    let icon_px = (WIN_BUTTON_ICON_SIZE_LOGICAL * scale_factor)
        .round()
        .max(1.0);
    let left = btn.left + (btn.width - icon_px) * 0.5;
    let top = btn.top + (btn.height - icon_px) * 0.5;
    let id = match kind {
        TitleButtonKind::Minimize => WIN_BTN_MINIMIZE_MASK_GLYPH_ID,
        TitleButtonKind::Maximize => {
            if maximized {
                WIN_BTN_RESTORE_MASK_GLYPH_ID
            } else {
                WIN_BTN_MAXIMIZE_MASK_GLYPH_ID
            }
        }
        TitleButtonKind::Close => WIN_BTN_CLOSE_MASK_GLYPH_ID,
    };
    out.push(CustomGlyph {
        id,
        left,
        top,
        width: icon_px,
        height: icon_px,
        color: Some(color),
        snap_to_physical_pixel: true,
        metadata: 0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_never_below_cell() {
        assert_eq!(title_bar_height_px(40.0, 1.0), 40.0);
        assert_eq!(title_bar_height_px(20.0, 1.0), 34.0);
    }

    #[test]
    fn buttons_fit_inside_surface() {
        let layout = compute_title_bar_layout(800.0, 8.0, 20.0, 1.0);
        assert!(layout.tab_area_width > 0.0);
        assert_eq!(layout.buttons.len(), 3);
        let last = layout.buttons.last().unwrap();
        assert!(last.left + last.width <= 800.0 - 8.0 + 0.01);
    }

    #[test]
    fn hit_test_detects_button() {
        let layout = compute_title_bar_layout(800.0, 8.0, 20.0, 1.0);
        let close = layout
            .buttons
            .iter()
            .find(|b| b.kind == TitleButtonKind::Close)
            .unwrap();
        let x = (close.left + close.width * 0.5) as f64;
        let y = (close.top + close.height * 0.5) as f64;
        assert_eq!(
            hit_test(&layout, x, y, 0.0),
            Some(TitleBarHit::Button(TitleButtonKind::Close))
        );
    }
}
