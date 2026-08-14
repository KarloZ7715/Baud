//! Histograma tecla→present. Solo vive si `diagnostics.latency_probe` es true;
//! el camino caliente por defecto no paga ni el `Instant::now()`.

use std::time::{Duration, Instant};

/// Percentiles de la sesión: se acumulan todas las muestras, no solo las
/// últimas N. El informe periódico no borra el histograma.
const PERIODIC_EVERY: u64 = 60;

pub struct LatencyTracker {
    epoch: Instant,
    /// Instante de la tecla más vieja que aún no se ha pintado.
    pending_key: Option<Duration>,
    samples_us: Vec<u64>,
    unlogged: u64,
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl LatencyTracker {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            pending_key: None,
            samples_us: Vec::new(),
            unlogged: 0,
        }
    }

    pub fn on_key(&mut self) {
        self.on_key_at(self.epoch.elapsed());
    }

    fn on_key_at(&mut self, now: Duration) {
        self.pending_key.get_or_insert(now);
    }

    pub fn on_present(&mut self) {
        self.on_present_at(self.epoch.elapsed());
    }

    fn on_present_at(&mut self, now: Duration) {
        if let Some(t0) = self.pending_key.take() {
            let us = now.saturating_sub(t0).as_micros() as u64;
            self.samples_us.push(us.max(1));
            self.unlogged += 1;
        }
    }

    pub fn samples(&self) -> u64 {
        self.samples_us.len() as u64
    }

    pub fn report(&self) -> String {
        let mut sorted = self.samples_us.clone();
        sorted.sort_unstable();
        format!(
            "latency key->present: n={} p50={} p90={} p99={} max={} (ms)",
            sorted.len(),
            ms_at_quantile(&sorted, 0.50),
            ms_at_quantile(&sorted, 0.90),
            ms_at_quantile(&sorted, 0.99),
            sorted.last().copied().unwrap_or(0) / 1000,
        )
    }

    /// Informe cada `PERIODIC_EVERY` muestras nuevas; no vacía el histograma.
    pub fn take_periodic(&mut self) -> Option<String> {
        if self.unlogged < PERIODIC_EVERY {
            return None;
        }
        self.unlogged = 0;
        Some(self.report())
    }
}

fn ms_at_quantile(sorted_us: &[u64], q: f64) -> u64 {
    if sorted_us.is_empty() {
        return 0;
    }
    let idx = (q * (sorted_us.len() as f64 - 1.0)).round() as usize;
    sorted_us[idx.min(sorted_us.len() - 1)] / 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn una_tecla_un_present_produce_una_muestra() {
        let mut t = LatencyTracker::new();
        t.on_key_at(Duration::from_millis(0));
        t.on_present_at(Duration::from_millis(7));
        assert_eq!(t.samples(), 1);
        assert!(t.report().contains("p50=7"));
    }

    #[test]
    fn presents_sin_tecla_pendiente_no_cuentan() {
        let mut t = LatencyTracker::new();
        t.on_present_at(Duration::from_millis(5));
        assert_eq!(t.samples(), 0);
    }

    #[test]
    fn dos_teclas_antes_del_present_cuentan_desde_la_primera() {
        // El usuario percibe la latencia de la tecla mas vieja sin pintar.
        let mut t = LatencyTracker::new();
        t.on_key_at(Duration::from_millis(0));
        t.on_key_at(Duration::from_millis(3));
        t.on_present_at(Duration::from_millis(10));
        assert_eq!(t.samples(), 1);
        assert!(t.report().contains("p50=10"));
    }

    #[test]
    fn informe_periodico_no_borra_muestras() {
        let mut t = LatencyTracker::new();
        for i in 0..PERIODIC_EVERY {
            t.on_key_at(Duration::from_millis(i));
            t.on_present_at(Duration::from_millis(i + 4));
        }
        let periodic = t.take_periodic().expect("60 muestras disparan informe");
        assert!(periodic.contains("n=60"));
        assert_eq!(t.samples(), 60);
        assert!(t.take_periodic().is_none());
    }
}
