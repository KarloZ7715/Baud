//! Selector interactivo de temas embebidos (preview en vivo + persistencia).

mod overlay;
mod samples;
mod style;

pub use overlay::{
    build_custom_glyphs, build_sample_custom_glyphs, configure_picker_buffers, fill_buffers,
    palette_layout, picker_cell_metrics, push_text_areas, PICKER_FONT_SIZE,
};
pub use samples::{build_sample_term, code_sample, prompt_sample, text_sample, SAMPLE_COLS};

use crate::color_scheme::SchemeSource;
use crate::config::{
    available_presets, preset_polarity, try_preset, ColorMode, ColorScheme, ThemeConfig,
};
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
    /// Modo de tema activo al abrir el picker (para mostrarlo en el panel).
    mode: ColorMode,
    /// Origen del esquema del SO (portal/winit/fallback) — info del panel.
    scheme_source: SchemeSource,
    /// Esquema del SO resuelto (`None` = sin señal, cae a oscuro).
    system_scheme: Option<ColorScheme>,
}

impl ThemePickerState {
    /// Abre el picker guardando el tema actual para restaurar al cancelar.
    pub fn open(
        theme: &ThemeConfig,
        active_preset: Option<&str>,
        saved_copy_mode: Option<CopyModeState>,
        mode: ColorMode,
        scheme_source: SchemeSource,
        system_scheme: Option<ColorScheme>,
    ) -> Self {
        let presets = available_presets();
        let mut filtered_indices: Vec<usize> = (0..presets.len()).collect();
        sort_by_polarity(&mut filtered_indices);
        let index = active_preset
            .and_then(|name| filtered_indices.iter().position(|&i| presets[i] == name))
            .unwrap_or(0);
        Self {
            saved_theme: theme.clone(),
            saved_preset: active_preset.map(str::to_string),
            saved_copy_mode,
            index,
            filter: String::new(),
            search_mode: false,
            filtered_indices,
            mode,
            scheme_source,
            system_scheme,
        }
    }

    /// Modo de tema activo (para mostrar en el panel).
    pub fn mode(&self) -> ColorMode {
        self.mode
    }

    /// Origen del esquema del SO (para mostrar en el panel cuando modo=auto).
    pub fn scheme_source(&self) -> SchemeSource {
        self.scheme_source
    }

    /// Esquema del SO resuelto al abrir el picker.
    pub fn system_scheme(&self) -> Option<ColorScheme> {
        self.system_scheme
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

    /// Número de presets oscuros en la lista filtrada (los claros van después).
    pub fn dark_count(&self) -> usize {
        let presets = available_presets();
        self.filtered_indices
            .iter()
            .filter(|&&i| preset_polarity(presets[i]) == ColorScheme::Dark)
            .count()
    }

    /// Fila (0-based, contando cabeceras de grupo) del preset seleccionado en
    /// la lista renderizada. `None` si la lista está vacía.
    ///
    /// El overlay usa este valor para posicionar el resaltado vertical, ya que
    /// las cabeceras "Dark"/"Light" desplazan los presets.
    pub fn selected_row(&self) -> Option<usize> {
        let presets = available_presets();
        let idx = *self.filtered_indices.get(self.index)?;
        let dark_count = self.dark_count();
        let has_dark = dark_count > 0;
        let has_light = self.filtered_indices.len() > dark_count;
        if preset_polarity(presets[idx]) == ColorScheme::Dark {
            Some(has_dark as usize + self.index)
        } else {
            Some(has_dark as usize + dark_count + has_light as usize + (self.index - dark_count))
        }
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
        // Agrupar por polaridad (oscuros primero, luego claros) preservando el
        // orden de registro dentro de cada grupo.
        sort_by_polarity(&mut self.filtered_indices);
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

/// Ordena índices de presets con los oscuros primero y los claros después,
/// preservando el orden relativo dentro de cada grupo (sort estable).
fn sort_by_polarity(indices: &mut [usize]) {
    let presets = available_presets();
    indices.sort_by_key(|&i| preset_polarity(presets[i]) == ColorScheme::Light);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::Term;
    use crate::config::ThemeConfig;

    #[test]
    fn filtro_por_substring() {
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        p.set_filter("drac");
        assert_eq!(p.filtered_presets(), vec!["dracula"]);
    }

    #[test]
    fn filtro_vacio_no_permite_confirmar() {
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        p.set_filter("zzz_sin_match");
        assert!(!p.can_confirm());
        assert!(p.try_selected_name().is_none());
        assert!(p.filtered_presets().is_empty());
    }

    #[test]
    fn enter_restaura_tema_guardado() {
        let theme = ThemeConfig::default();
        let saved_bg = theme.background.clone();
        let mut p = ThemePickerState::open(
            &theme,
            Some("nord"),
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        p.set_filter("dracula");
        assert_eq!(p.try_selected_name(), Some("dracula"));
        assert_ne!(p.preview_theme().background, saved_bg);
        assert_eq!(p.saved_theme().background, saved_bg);
    }

    #[test]
    fn navegacion_circular() {
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        let first = p.try_selected_name().unwrap();
        let count = p.filtered_presets().len();
        for _ in 0..count {
            p.move_next();
        }
        assert_eq!(p.try_selected_name(), Some(first));
    }

    #[test]
    fn filtro_vacio_muestra_todos() {
        let p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        assert_eq!(p.filtered_presets().len(), available_presets().len());
    }

    #[test]
    fn preview_usa_preset_seleccionado() {
        let p = ThemePickerState::open(
            &ThemeConfig::default(),
            Some("dracula"),
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        let t = p.preview_theme();
        assert_eq!(t.background, try_preset("dracula").unwrap().background);
    }

    #[test]
    fn restaura_copy_mode_guardado() {
        let term = Term::new();
        let cm = CopyModeState::enter(&term);
        let p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            Some(cm),
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        assert_eq!(p.saved_copy_mode(), Some(cm));
    }

    #[test]
    fn commit_search_conserva_filtro() {
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
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
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        p.set_filter("dark");
        let names = p.filtered_presets();
        assert!(names.len() > 1, "debe haber varios presets con 'dark'");
        p.move_next();
        assert_ne!(p.try_selected_name(), Some(names[0]));
    }

    #[test]
    fn presets_agrupados_por_polaridad_oscuros_primero() {
        let p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        let names = p.filtered_presets();
        // 16 oscuros + 6 claros = 22.
        assert_eq!(names.len(), available_presets().len());
        assert_eq!(p.dark_count(), 16);
        // Los primeros 16 son oscuros, los últimos 6 claros.
        for name in &names[..16] {
            assert_eq!(preset_polarity(name), ColorScheme::Dark);
        }
        for name in &names[16..] {
            assert_eq!(preset_polarity(name), ColorScheme::Light);
        }
    }

    #[test]
    fn selected_row_cuenta_cabeceras_de_grupo() {
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        // Cabecera "dark" en fila 0 => primer preset oscuro en fila 1.
        p.move_home();
        assert_eq!(p.selected_row(), Some(1));
        // Último preset (claro, índice 21): dark(16) + 2 cabeceras + 5 = 23.
        p.move_end();
        assert_eq!(p.selected_row(), Some(1 + 16 + 1 + 5));
    }

    #[test]
    fn filtro_solo_light_sin_cabecera_dark() {
        let mut p = ThemePickerState::open(
            &ThemeConfig::default(),
            None,
            None,
            ColorMode::Dark,
            SchemeSource::Fallback,
            None,
        );
        p.set_filter("light");
        assert_eq!(p.dark_count(), 0);
        // Sin oscuros: solo cabecera "light" (fila 0) => primer claro en fila 1.
        p.move_home();
        assert_eq!(p.selected_row(), Some(1));
        assert_eq!(
            preset_polarity(p.try_selected_name().unwrap()),
            ColorScheme::Light
        );
    }
}
