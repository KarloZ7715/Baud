use winit::event::MouseScrollDelta;

/// Limite de lineas por evento para evitar saltos patologicos.
const MAX_LINES_PER_EVENT: f32 = 50.0;

/// Indica si la rueda pertenece a la app (reporting activo) o al host (Baud).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelOwnerHint {
    App,
    Host,
}

/// Intento resuelto por la politica de rueda tras clasificar delta y contexto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelIntent {
    /// Sin accion (delta nulo o sin lineas tras acumulacion).
    None,
    /// Reenviar boton de rueda a la app (64 = up, 65 = down).
    ForwardReport { button: u8 },
    /// Ajustar scrollback local (positivo = hacia historia, negativo = hacia borde vivo).
    LocalLines(isize),
    /// Enviar teclas de cursor sinteticas al PTY en pantalla alterna.
    FauxLines { up: bool, count: u16 },
}

/// Convierte el delta de winit a lineas flotantes acumuladas.
///
/// `cell_height_px` convierte PixelDelta a lineas. Si es 0.0,
/// se usa una heuristica fija de ~16 px por celda.
///
/// `residual` acumula fracciones entre eventos para trackpads.
/// Retorna las lineas enteras disponibles tras la acumulacion.
pub fn lines_from_delta(delta: &MouseScrollDelta, cell_height_px: f32, residual: &mut f32) -> f32 {
    let raw = match delta {
        MouseScrollDelta::LineDelta(_, y) => *y,
        MouseScrollDelta::PixelDelta(pos) => {
            let cell_h = if cell_height_px > 0.0 {
                cell_height_px
            } else {
                16.0
            };
            pos.y as f32 / cell_h
        }
    };
    if raw == 0.0 {
        return 0.0;
    }
    *residual += raw;
    let whole = residual.trunc();
    *residual -= whole;
    whole
}

/// Resuelve el intento de rueda a partir de lineas enteras y contexto.
///
/// `lines` > 0 = scroll up (hacia historia, boton 64); < 0 = scroll down.
/// `multiplier` escala lineas locales; `faux_multiplier` escala flechas sinteticas.
pub fn resolve(
    owner: WheelOwnerHint,
    alt_screen: bool,
    lines: f32,
    multiplier: f32,
    faux_multiplier: f32,
) -> WheelIntent {
    if lines == 0.0 {
        return WheelIntent::None;
    }
    match owner {
        WheelOwnerHint::App => {
            let button = if lines > 0.0 { 64 } else { 65 };
            WheelIntent::ForwardReport { button }
        }
        WheelOwnerHint::Host => {
            if alt_screen {
                let up = lines > 0.0;
                let scaled = lines.abs() * faux_multiplier;
                let count = (scaled as u16).min(MAX_LINES_PER_EVENT as u16);
                if count == 0 {
                    WheelIntent::None
                } else {
                    WheelIntent::FauxLines { up, count }
                }
            } else {
                let scaled = lines * multiplier;
                let clamped = scaled
                    .round()
                    .clamp(-MAX_LINES_PER_EVENT, MAX_LINES_PER_EVENT)
                    as isize;
                if clamped == 0 {
                    WheelIntent::None
                } else {
                    WheelIntent::LocalLines(clamped)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ld(y: f32) -> MouseScrollDelta {
        MouseScrollDelta::LineDelta(0.0, y)
    }

    fn pd(y: f32) -> MouseScrollDelta {
        MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition {
            x: 0.0,
            y: y as f64,
        })
    }

    #[test]
    fn line_delta_up_with_multiplier_3_gives_local_lines_3() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, false, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::LocalLines(3));
    }

    #[test]
    fn line_delta_down_gives_local_lines_negative() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(-1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, false, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::LocalLines(-3));
    }

    #[test]
    fn host_alt_screen_produces_faux_lines_not_local() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, true, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::FauxLines { up: true, count: 3 });
    }

    #[test]
    fn host_alt_screen_down_produces_faux_down() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(-1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, true, lines, 3.0, 3.0);
        assert_eq!(
            intent,
            WheelIntent::FauxLines {
                up: false,
                count: 3
            }
        );
    }

    #[test]
    fn app_owner_returns_forward_report_regardless_of_alt_screen() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::App, false, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::ForwardReport { button: 64 });
    }

    #[test]
    fn app_owner_down_returns_button_65() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(-1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::App, true, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::ForwardReport { button: 65 });
    }

    #[test]
    fn pixel_delta_accumulation_two_half_events_gives_one_line() {
        let mut r = 0.0;
        let cell_h = 20.0;
        let a = lines_from_delta(&pd(10.0), cell_h, &mut r);
        let b = lines_from_delta(&pd(10.0), cell_h, &mut r);
        assert_eq!(a, 0.0);
        assert_eq!(b, 1.0);
    }

    #[test]
    fn residual_does_not_double_apply() {
        let mut r = 0.0;
        let cell_h = 20.0;
        // Primer delta: 25 px / 20 = 1.25 → 1 linea entera, residual 0.25
        let a = lines_from_delta(&pd(25.0), cell_h, &mut r);
        assert_eq!(a, 1.0);
        // Segundo delta: 25 px / 20 = 1.25 + 0.25 = 1.50 → 1 linea, residual 0.50
        let b = lines_from_delta(&pd(25.0), cell_h, &mut r);
        assert_eq!(b, 1.0);
        // Tercer delta: 25 px / 20 = 1.25 + 0.50 = 1.75 → 1 linea, residual 0.75
        let c = lines_from_delta(&pd(25.0), cell_h, &mut r);
        assert_eq!(c, 1.0);
        // Cuarto delta: 25 px / 20 = 1.25 + 0.75 = 2.00 → 2 lineas, residual 0.0
        let d = lines_from_delta(&pd(25.0), cell_h, &mut r);
        assert_eq!(d, 2.0);
        assert!((r - 0.0).abs() < f32::EPSILON * 10.0);
    }

    #[test]
    fn clamp_huge_delta_to_max_lines() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(100.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, false, lines, 3.0, 3.0);
        // 100 * 3 = 300, clamped to 50
        assert_eq!(intent, WheelIntent::LocalLines(50));
    }

    #[test]
    fn clamp_huge_negative_delta() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(-100.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, false, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::LocalLines(-50));
    }

    #[test]
    fn multiplier_1_gives_one_line_per_unit() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, false, lines, 1.0, 1.0);
        assert_eq!(intent, WheelIntent::LocalLines(1));
    }

    #[test]
    fn zero_delta_gives_none() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(0.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, false, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::None);
    }

    #[test]
    fn pixel_delta_negative_accumulates_correctly() {
        let mut r = 0.0;
        let cell_h = 20.0;
        let a = lines_from_delta(&pd(-15.0), cell_h, &mut r);
        let b = lines_from_delta(&pd(-15.0), cell_h, &mut r);
        assert_eq!(a, 0.0);
        assert_eq!(b, -1.0);
        // Residual -0.5 (=-15/20-15/20 = -1.5, trunc = -1, residual = -0.5)
        // Verificar que el residual es aproximadamente -0.5
        assert!((r - (-0.5)).abs() < f32::EPSILON * 10.0);
    }

    #[test]
    fn app_owner_with_alt_screen_still_forwards() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(1.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::App, true, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::ForwardReport { button: 64 });
    }

    #[test]
    fn faux_multiplier_scales_count() {
        let mut r = 0.0;
        let lines = lines_from_delta(&ld(2.0), 20.0, &mut r);
        let intent = resolve(WheelOwnerHint::Host, true, lines, 3.0, 5.0);
        // 2 lines * 5 faux_multiplier = 10 faux arrows
        assert_eq!(
            intent,
            WheelIntent::FauxLines {
                up: true,
                count: 10
            }
        );
    }

    #[test]
    fn cell_height_zero_falls_back_to_heuristic() {
        let mut r = 0.0;
        let lines = lines_from_delta(&pd(32.0), 0.0, &mut r);
        // 32 px / 16 (heuristic) = 2.0 lines
        assert_eq!(lines, 2.0);
    }

    #[test]
    fn small_faux_count_rounds_to_zero_triggers_none() {
        let mut r = 0.0;
        let lines = lines_from_delta(&pd(2.0), 20.0, &mut r);
        // 2 px / 20 cell_h * 3 faux_multiplier = 0.3 → 0 count
        let intent = resolve(WheelOwnerHint::Host, true, lines, 3.0, 3.0);
        assert_eq!(intent, WheelIntent::None);
    }
}
