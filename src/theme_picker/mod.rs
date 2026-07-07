//! Selector interactivo de temas embebidos (preview en vivo + persistencia).

mod overlay;
mod samples;
mod style;

pub use overlay::{
    build_custom_glyphs, build_sample_custom_glyphs, configure_picker_buffers, fill_buffers,
    palette_layout, picker_cell_metrics, push_text_areas, PICKER_FONT_SIZE,
};
pub use samples::{build_sample_term, code_sample, prompt_sample, text_sample, SAMPLE_COLS};

use crate::config::{available_presets, try_preset, ThemeConfig};
use crate::copy_mode::CopyModeState;

const PAGE_STEP: usize = 10;

/// Estado del theme picker (vive en `App`, no en `Term`).
#[derive(Debug, Clone)]
pub struct ThemePickerState {
    saved_theme: ThemeConfig,
    saved_preset: Option<String>,
    /// Copy mode activo al abrir el picker (se restaura al cancelar).
    saved_copy_mode: Option<CopyModeState>,
    /// Índice en `filtered_indices`.
    index: usize,
    filter: String,
    /// Modo búsqueda activo (`/`).
    pub search_mode: bool,
    filtered_indices: Vec<usize>,
}

impl ThemePickerState {
    /// Abre el picker guardando el tema actual para restaurar al cancelar.
    pub fn open(
        theme: &ThemeConfig,
        active_preset: Option<&str>,
        saved_copy_mode: Option<CopyModeState>,
    ) -> Self {
        let presets = available_presets();
        let filtered_indices: Vec<usize> = (0..presets.len()).collect();
        let index = active_preset
            .and_then(|name| presets.iter().position(|p| *p == name))
            .unwrap_or(0);
        Self {
            saved_theme: theme.clone(),
            saved_preset: active_preset.map(str::to_string),
            saved_copy_mode,
            index,
            filter: String::new(),
            search_mode: false,
            filtered_indices,
        }
    }

    pub fn saved_theme(&self) -> &ThemeConfig {
        &self.saved_theme
    }

    pub fn saved_preset(&self) -> Option<&str> {
        self.saved_preset.as_deref()
    }

    pub fn saved_copy_mode(&self) -> Option<CopyModeState> {
        self.saved_copy_mode
    }

    /// Hay un preset seleccionable (lista filtrada no vacía).
    pub fn can_confirm(&self) -> bool {
        !self.filtered_indices.is_empty()
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn is_search_mode(&self) -> bool {
        self.search_mode
    }

    /// Presets visibles tras aplicar el filtro.
    pub fn filtered_presets(&self) -> Vec<&'static str> {
        let presets = available_presets();
        self.filtered_indices.iter().map(|&i| presets[i]).collect()
    }

    /// Nombre del preset seleccionado en la lista filtrada.
    pub fn try_selected_name(&self) -> Option<&'static str> {
        let presets = available_presets();
        self.filtered_indices
            .get(self.index)
            .map(|&idx| presets[idx])
    }

    /// Tema del preset en preview.
    pub fn preview_theme(&self) -> ThemeConfig {
        self.try_selected_name()
            .and_then(|name| try_preset(name).ok())
            .unwrap_or_else(|| self.saved_theme.clone())
    }

    pub fn move_next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.index = (self.index + 1) % self.filtered_indices.len();
    }

    pub fn move_prev(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.index = self
            .index
            .checked_sub(1)
            .unwrap_or(self.filtered_indices.len() - 1);
    }

    pub fn page_down(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let len = self.filtered_indices.len();
        self.index = (self.index + PAGE_STEP.min(len)).min(len - 1);
    }

    pub fn page_up(&mut self) {
        self.index = self.index.saturating_sub(PAGE_STEP);
    }

    pub fn move_home(&mut self) {
        self.index = 0;
    }

    pub fn move_end(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.index = self.filtered_indices.len() - 1;
        }
    }

    pub fn start_search(&mut self) {
        self.search_mode = true;
        self.filter.clear();
        self.rebuild_filter();
    }

    pub fn cancel_search(&mut self) {
        self.search_mode = false;
        self.filter.clear();
        self.rebuild_filter();
    }

    /// Sale del modo búsqueda conservando el filtro activo.
    pub fn commit_search(&mut self) {
        self.search_mode = false;
    }

    /// Hay un filtro aplicado (sin estar escribiendo).
    pub fn has_active_filter(&self) -> bool {
        !self.search_mode && !self.filter.is_empty()
    }

    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.rebuild_filter();
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.rebuild_filter();
    }

    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
        self.rebuild_filter();
    }

    fn rebuild_filter(&mut self) {
        let presets = available_presets();
        let needle = self.filter.to_ascii_lowercase();
        let prev_name = self.try_selected_name();
        if needle.is_empty() {
            self.filtered_indices = (0..presets.len()).collect();
        } else {
            self.filtered_indices = presets
                .iter()
                .enumerate()
                .filter(|(_, name)| name.to_ascii_lowercase().contains(&needle))
                .map(|(i, _)| i)
                .collect();
        }
        if self.filtered_indices.is_empty() {
            self.index = 0;
            return;
        }
        self.index = prev_name
            .and_then(|prev| {
                self.filtered_indices
                    .iter()
                    .position(|&i| presets[i] == prev)
            })
            .unwrap_or(0)
            .min(self.filtered_indices.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::Term;
    use crate::config::ThemeConfig;

    #[test]
    fn filtro_por_substring() {
        let mut p = ThemePickerState::open(&ThemeConfig::default(), None, None);
        p.set_filter("drac");
        assert_eq!(p.filtered_presets(), vec!["dracula"]);
    }

    #[test]
    fn filtro_vacio_no_permite_confirmar() {
        let mut p = ThemePickerState::open(&ThemeConfig::default(), None, None);
        p.set_filter("zzz_sin_match");
        assert!(!p.can_confirm());
        assert!(p.try_selected_name().is_none());
        assert!(p.filtered_presets().is_empty());
    }

    #[test]
    fn enter_restaura_tema_guardado() {
        let theme = ThemeConfig::default();
        let saved_bg = theme.background.clone();
        let mut p = ThemePickerState::open(&theme, Some("nord"), None);
        p.set_filter("dracula");
        assert_eq!(p.try_selected_name(), Some("dracula"));
        assert_ne!(p.preview_theme().background, saved_bg);
        assert_eq!(p.saved_theme().background, saved_bg);
    }

    #[test]
    fn navegacion_circular() {
        let mut p = ThemePickerState::open(&ThemeConfig::default(), None, None);
        let first = p.try_selected_name().unwrap();
        let count = p.filtered_presets().len();
        for _ in 0..count {
            p.move_next();
        }
        assert_eq!(p.try_selected_name(), Some(first));
    }

    #[test]
    fn filtro_vacio_muestra_todos() {
        let p = ThemePickerState::open(&ThemeConfig::default(), None, None);
        assert_eq!(p.filtered_presets().len(), available_presets().len());
    }

    #[test]
    fn preview_usa_preset_seleccionado() {
        let p = ThemePickerState::open(&ThemeConfig::default(), Some("dracula"), None);
        let t = p.preview_theme();
        assert_eq!(t.background, try_preset("dracula").unwrap().background);
    }

    #[test]
    fn restaura_copy_mode_guardado() {
        let term = Term::new();
        let cm = CopyModeState::enter(&term);
        let p = ThemePickerState::open(&ThemeConfig::default(), None, Some(cm));
        assert_eq!(p.saved_copy_mode(), Some(cm));
    }

    #[test]
    fn commit_search_conserva_filtro() {
        let mut p = ThemePickerState::open(&ThemeConfig::default(), None, None);
        p.start_search();
        p.push_filter_char('d');
        p.push_filter_char('r');
        p.push_filter_char('a');
        assert!(p.is_search_mode());
        assert_eq!(p.filtered_presets(), vec!["dracula"]);
        p.commit_search();
        assert!(!p.is_search_mode());
        assert!(p.has_active_filter());
        assert_eq!(p.filter(), "dra");
        assert_eq!(p.filtered_presets(), vec!["dracula"]);
    }

    #[test]
    fn filtro_dark_muestra_varios() {
        let mut p = ThemePickerState::open(&ThemeConfig::default(), None, None);
        p.set_filter("dark");
        let names = p.filtered_presets();
        assert!(names.len() > 1, "debe haber varios presets con 'dark'");
        p.move_next();
        assert_ne!(p.try_selected_name(), Some(names[0]));
    }
}
