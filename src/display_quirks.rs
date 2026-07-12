//! Detección de backend de display y tabla de quirks por familia de compositor.
//!
//! El backend (Wayland/X11) se obtiene de winit tras construir el event loop.
//! Las variables de entorno solo afinan la familia del compositor.

use std::env;

/// Backend de presentación detectado por winit (o desconocido fuera de Unix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayBackend {
    Wayland,
    X11,
    Other,
}

/// Familia de compositor / escritorio inferida de señales suaves de entorno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorFamily {
    Hyprland,
    Wlroots,
    Gnome,
    Kde,
    Unknown,
}

/// Instantánea de comportamientos dependientes de sesión. Se resuelve una vez
/// al arranque; no se consulta por frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayQuirks {
    pub backend: DisplayBackend,
    pub family: CompositorFamily,
    /// Pedir un redraw inicial para que la superficie se presente.
    pub force_initial_redraw: bool,
    /// Tras salir el cursor, el backend deja de emitir movimiento.
    pub cursor_left_stops_moved: bool,
    /// Probabilidad de selección primaria útil (aviso para clipboard).
    pub primary_selection_likely: bool,
}

impl DisplayQuirks {
    /// Valores seguros antes de detectar el event loop.
    pub const DEFAULT: Self = Self {
        backend: DisplayBackend::Other,
        family: CompositorFamily::Unknown,
        force_initial_redraw: false,
        cursor_left_stops_moved: false,
        primary_selection_likely: false,
    };
}

/// Detecta el backend a partir de un [`ActiveEventLoop`] de winit.
///
/// En Linux/BSD usa `ActiveEventLoopExtWayland` / `ActiveEventLoopExtX11`.
/// En otras plataformas devuelve [`DisplayBackend::Other`].
pub fn detect_backend(event_loop: &winit::event_loop::ActiveEventLoop) -> DisplayBackend {
    detect_backend_from_flags(is_wayland_active(event_loop), is_x11_active(event_loop))
}

fn detect_backend_from_flags(wayland: bool, x11: bool) -> DisplayBackend {
    match (wayland, x11) {
        (true, _) => DisplayBackend::Wayland,
        (false, true) => DisplayBackend::X11,
        (false, false) => DisplayBackend::Other,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_wayland_active(event_loop: &winit::event_loop::ActiveEventLoop) -> bool {
    use winit::platform::wayland::ActiveEventLoopExtWayland;
    event_loop.is_wayland()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_x11_active(event_loop: &winit::event_loop::ActiveEventLoop) -> bool {
    use winit::platform::x11::ActiveEventLoopExtX11;
    event_loop.is_x11()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn is_wayland_active(_event_loop: &winit::event_loop::ActiveEventLoop) -> bool {
    false
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn is_x11_active(_event_loop: &winit::event_loop::ActiveEventLoop) -> bool {
    false
}

#[derive(Debug, Clone, Copy)]
struct EnvHints<'a> {
    hyprland_signature: Option<&'a str>,
    xdg_current_desktop: Option<&'a str>,
    desktop_session: Option<&'a str>,
    wayland_display: Option<&'a str>,
}

fn detect_compositor_family_from_env(hints: EnvHints<'_>) -> CompositorFamily {
    if hints.hyprland_signature.is_some_and(|v| !v.is_empty()) {
        return CompositorFamily::Hyprland;
    }

    let desktop = hints
        .xdg_current_desktop
        .or(hints.desktop_session)
        .unwrap_or("")
        .to_ascii_lowercase();

    if desktop.contains("hyprland") {
        return CompositorFamily::Hyprland;
    }
    if desktop.contains("sway") || desktop.contains("wlroots") {
        return CompositorFamily::Wlroots;
    }
    if desktop.contains("gnome") {
        return CompositorFamily::Gnome;
    }
    if desktop.contains("kde") || desktop.contains("plasma") {
        return CompositorFamily::Kde;
    }

    // Sin señales de escritorio: no inventar familia solo por WAYLAND_DISPLAY.
    let _ = hints.wayland_display;
    CompositorFamily::Unknown
}

/// Resuelve quirks a partir de backend + familia. Coste O(1), una vez al arranque.
pub fn resolve_quirks(backend: DisplayBackend, family: CompositorFamily) -> DisplayQuirks {
    let force_initial_redraw = match family {
        // Familia con hung-window documentado si no hay present temprano.
        CompositorFamily::Hyprland => true,
        // En Wayland la superficie no aparece hasta el primer present (winit).
        _ => matches!(backend, DisplayBackend::Wayland),
    };

    let cursor_left_stops_moved = matches!(backend, DisplayBackend::Wayland);

    // Aviso para clipboard: X11 suele tener PRIMARY; wlroots/Hyprland también.
    // GNOME Wayland a menudo no expone primary de forma útil.
    let primary_selection_likely = match (backend, family) {
        (DisplayBackend::X11, _) => true,
        (DisplayBackend::Wayland, CompositorFamily::Hyprland | CompositorFamily::Wlroots) => true,
        (DisplayBackend::Wayland, CompositorFamily::Gnome) => false,
        (DisplayBackend::Wayland, CompositorFamily::Kde | CompositorFamily::Unknown) => true,
        _ => false,
    };

    DisplayQuirks {
        backend,
        family,
        force_initial_redraw,
        cursor_left_stops_moved,
        primary_selection_likely,
    }
}

/// Detecta backend + familia y resuelve la instantánea completa.
pub fn snapshot_for_event_loop(event_loop: &winit::event_loop::ActiveEventLoop) -> DisplayQuirks {
    let backend = detect_backend(event_loop);
    let family = detect_family_from_process_env();
    let quirks = resolve_quirks(backend, family);
    tracing::info!(
        backend = ?quirks.backend,
        family = ?quirks.family,
        force_initial_redraw = quirks.force_initial_redraw,
        cursor_left_stops_moved = quirks.cursor_left_stops_moved,
        primary_selection_likely = quirks.primary_selection_likely,
        "display quirks resueltos"
    );
    quirks
}

fn detect_family_from_process_env() -> CompositorFamily {
    let hypr = env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    let xdg = env::var("XDG_CURRENT_DESKTOP").ok();
    let session = env::var("DESKTOP_SESSION").ok();
    let wayland = env::var("WAYLAND_DISPLAY").ok();

    detect_compositor_family_from_env(EnvHints {
        hyprland_signature: hypr.as_deref(),
        xdg_current_desktop: xdg.as_deref(),
        desktop_session: session.as_deref(),
        wayland_display: wayland.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_fuerza_redraw_inicial() {
        let q = resolve_quirks(DisplayBackend::Wayland, CompositorFamily::Hyprland);
        assert!(q.force_initial_redraw);
        assert!(q.cursor_left_stops_moved);
        assert!(q.primary_selection_likely);
    }

    #[test]
    fn x11_desconocido_no_fuerza_redraw() {
        let q = resolve_quirks(DisplayBackend::X11, CompositorFamily::Unknown);
        assert!(!q.force_initial_redraw);
        assert!(!q.cursor_left_stops_moved);
        assert!(q.primary_selection_likely);
    }

    #[test]
    fn wayland_desconocido_fuerza_redraw_por_present() {
        let q = resolve_quirks(DisplayBackend::Wayland, CompositorFamily::Unknown);
        assert!(q.force_initial_redraw);
        assert!(q.cursor_left_stops_moved);
    }

    #[test]
    fn gnome_wayland_sin_primary_probable() {
        let q = resolve_quirks(DisplayBackend::Wayland, CompositorFamily::Gnome);
        assert!(q.force_initial_redraw);
        assert!(!q.primary_selection_likely);
    }

    #[test]
    fn familia_hyprland_por_signature() {
        let family = detect_compositor_family_from_env(EnvHints {
            hyprland_signature: Some("abc123"),
            xdg_current_desktop: Some("GNOME"),
            desktop_session: None,
            wayland_display: Some("wayland-1"),
        });
        assert_eq!(family, CompositorFamily::Hyprland);
    }

    #[test]
    fn familia_desde_xdg_desktop() {
        assert_eq!(
            detect_compositor_family_from_env(EnvHints {
                hyprland_signature: None,
                xdg_current_desktop: Some("ubuntu:GNOME"),
                desktop_session: None,
                wayland_display: Some("wayland-0"),
            }),
            CompositorFamily::Gnome
        );
        assert_eq!(
            detect_compositor_family_from_env(EnvHints {
                hyprland_signature: None,
                xdg_current_desktop: Some("KDE"),
                desktop_session: Some("plasma"),
                wayland_display: None,
            }),
            CompositorFamily::Kde
        );
        assert_eq!(
            detect_compositor_family_from_env(EnvHints {
                hyprland_signature: None,
                xdg_current_desktop: Some("sway"),
                desktop_session: None,
                wayland_display: Some("wayland-1"),
            }),
            CompositorFamily::Wlroots
        );
    }

    #[test]
    fn sin_pistas_familia_unknown() {
        let family = detect_compositor_family_from_env(EnvHints {
            hyprland_signature: None,
            xdg_current_desktop: None,
            desktop_session: None,
            wayland_display: Some("wayland-0"),
        });
        assert_eq!(family, CompositorFamily::Unknown);
    }

    #[test]
    fn backend_flags_priorizan_wayland() {
        assert_eq!(
            detect_backend_from_flags(true, true),
            DisplayBackend::Wayland
        );
        assert_eq!(detect_backend_from_flags(false, true), DisplayBackend::X11);
        assert_eq!(
            detect_backend_from_flags(false, false),
            DisplayBackend::Other
        );
    }

    #[test]
    fn default_quirks_seguros() {
        let q = DisplayQuirks::DEFAULT;
        assert!(!q.force_initial_redraw);
        assert_eq!(q.backend, DisplayBackend::Other);
        assert_eq!(q.family, CompositorFamily::Unknown);
    }
}
