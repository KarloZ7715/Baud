//! Parpadeo del cursor y texto SGR 5.
//!
//! El cursor y las celdas con SGR 5 (blink) se muestran durante la primera
//! mitad del intervalo y se ocultan durante la segunda mitad, como en xterm.
//! Cualquier entrada del usuario o salida del PTY resetea la fase a "on" via
//! `Term::last_blink_reset`, de modo que el cursor no parpadea mientras se
//! escribe.

use std::time::Duration;

/// Indica si la fase actual es "visible" dado el tiempo transcurrido desde el
/// ultimo reset y la duracion total del intervalo de parpadeo.
///
/// Devuelve `true` (siempre visible) cuando el intervalo es cero: el parpadeo
/// esta desactivado y no tiene sentido alternar la fase.
pub fn blink_on(elapsed: Duration, interval: Duration) -> bool {
    if interval.is_zero() {
        return true;
    }
    let interval_ms = interval.as_millis().max(1);
    let half = interval_ms / 2;
    let pos = elapsed.as_millis() % interval_ms;
    pos < half
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
}
