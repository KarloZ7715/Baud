//! Parpadeo del cursor y texto SGR 5.
//!
//! Tras un reset de fase (entrada del usuario o salida del PTY) el cursor y
//! el texto SGR 5 quedan visibles durante un intervalo completo. Pasada esa
//! supresion, la fase alterna: visible la primera mitad del intervalo, oculta
//! la segunda. `blink_interval_ms == 0` desactiva el parpadeo y todo queda
//! visible.

use std::time::Duration;

/// Fase del ciclo (sin supresion): visible la primera mitad del intervalo.
///
/// Devuelve `true` (siempre visible) cuando el intervalo es cero: el parpadeo
/// esta desactivado y no tiene sentido alternar la fase.
pub fn blink_on(elapsed: Duration, interval: Duration) -> bool {
    if interval.is_zero() {
        return true;
    }
    let interval_ms = interval.as_millis().max(1);
    let half = (interval_ms / 2).max(1);
    let pos = elapsed.as_millis() % interval_ms;
    pos < half
}

/// Visible durante la supresion posterior al reset y, despues, segun `blink_on`.
pub fn blink_visible(elapsed: Duration, interval: Duration) -> bool {
    if blink_suppressed(elapsed, interval) {
        return true;
    }
    blink_on(elapsed.saturating_sub(interval), interval)
}

/// Un intervalo completo tras el ultimo reset: el timer no debe pedir redraw.
pub fn blink_suppressed(elapsed: Duration, interval: Duration) -> bool {
    !interval.is_zero() && elapsed < interval
}

/// Tiempo hasta el proximo cambio visual de fase, o `None` si no hay que
/// programar timer (intervalo cero o ventana sin foco).
///
/// El primer cambio real ocurre al terminar la mitad "on" posterior a la
/// supresion (`interval + half`), no al caer la supresion: ahi el cursor
/// sigue visible.
pub fn blink_next_deadline(
    elapsed: Duration,
    interval: Duration,
    window_focused: bool,
) -> Option<Duration> {
    if !window_focused || interval.is_zero() {
        return None;
    }
    let interval_ms = interval.as_millis().max(1);
    let half = (interval_ms / 2).max(1);
    let first_toggle = interval.saturating_add(Duration::from_millis(half as u64));
    if elapsed < first_toggle {
        return Some(first_toggle - elapsed);
    }
    let pos = elapsed.saturating_sub(interval).as_millis() % interval_ms;
    let remaining_ms = if pos < half {
        half - pos
    } else {
        interval_ms - pos
    };
    Some(Duration::from_millis(remaining_ms as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fase_on_durante_primera_mitad() {
        let interval = Duration::from_millis(1000);
        assert!(blink_on(Duration::from_millis(0), interval));
        assert!(blink_on(Duration::from_millis(400), interval));
        assert!(!blink_on(Duration::from_millis(600), interval));
        assert!(blink_on(Duration::from_millis(1100), interval));
    }

    #[test]
    fn intervalo_cero_siempre_visible() {
        let zero = Duration::ZERO;
        assert!(blink_on(Duration::from_millis(0), zero));
        assert!(blink_on(Duration::from_millis(500), zero));
    }

    #[test]
    fn borde_exacto_mitad_es_off() {
        let interval = Duration::from_millis(1000);
        assert!(!blink_on(Duration::from_millis(500), interval));
        assert!(blink_on(Duration::from_millis(499), interval));
    }

    /// Intervalos degenerados (1ms) no deben dejar la fase permanentemente
    /// off: `half` se clampa a >= 1 para garantizar una ventana visible.
    #[test]
    fn intervalo_muy_corto_no_deja_siempre_off() {
        let interval = Duration::from_millis(1);
        assert!(blink_on(Duration::from_millis(0), interval));
        assert!(blink_on(Duration::from_millis(1), interval));
        assert!(blink_on(Duration::from_millis(2), interval));
        assert!(blink_on(Duration::from_millis(3), interval));
    }

    #[test]
    fn teclear_deja_el_cursor_visible_y_resetea_fase() {
        let interval = Duration::from_millis(1000);
        assert!(!blink_on(Duration::from_millis(600), interval));
        assert!(blink_visible(Duration::ZERO, interval));
        assert!(blink_visible(Duration::from_millis(1), interval));
        assert!(blink_visible(Duration::from_millis(999), interval));
    }

    #[test]
    fn el_blink_vuelve_tras_la_inactividad() {
        let interval = Duration::from_millis(1000);
        // Al terminar la supresion el ciclo arranca de cero: on 0-500, off 500-1000.
        assert!(blink_visible(Duration::from_millis(1000), interval));
        assert!(blink_visible(Duration::from_millis(1400), interval));
        assert!(!blink_visible(Duration::from_millis(1600), interval));
    }

    #[test]
    fn intervalo_cero_no_supprime_ni_programa_timer() {
        assert!(blink_visible(Duration::from_millis(10_000), Duration::ZERO));
        assert!(!blink_suppressed(Duration::from_millis(10), Duration::ZERO));
        assert!(blink_next_deadline(Duration::ZERO, Duration::ZERO, true).is_none());
    }

    #[test]
    fn sin_foco_no_se_programa_redraw() {
        let interval = Duration::from_millis(1000);
        assert!(blink_next_deadline(Duration::from_millis(100), interval, false).is_none());
    }

    #[test]
    fn con_foco_el_timer_espera_al_primer_apagado() {
        let interval = Duration::from_millis(1000);
        // Primer cambio visual: interval + half = 1500ms.
        assert_eq!(
            blink_next_deadline(Duration::from_millis(200), interval, true),
            Some(Duration::from_millis(1300))
        );
    }
}
