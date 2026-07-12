---
titulo: "Matriz de sesiones Linux"
tipo: guia
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-07-11"
fecha_modificacion: "2026-07-11"
version: "0.1.0"
estado: borrador
tags: [linux, wayland, x11, smoke, quirks]
iteracion: 1
---

# Matriz de sesiones Linux

| Version | Fecha      | Cambios                                      |
| ------- | ---------- | -------------------------------------------- |
| 0.1.0   | 2026-07-11 | Checklist inicial + límites de xvfb en CI    |

## Resumen

Baud debe comportarse de forma predecible en sesiones Linux reales. Los quirks de display viven en `src/display_quirks.rs`; esta guía es la matriz de smoke para contribuidores.

**Paridad de feeling:** la ventana mapea, acepta ASCII, redimensiona y cierra. Pegar clipboard solo si el backend de clipboard lo permite.

## Límites de CI (xvfb)

El job opcional `xvfb-smoke` en `.github/workflows/ci.yml` es **solo X11**:

- Arranca bajo `xvfb-run` con `WAYLAND_DISPLAY` y `WAYLAND_SOCKET` vacíos.
- Demuestra que el binario no crashea al lanzar en un display X11 sintético.
- **No** cubre Wayland, ni familias de compositor concretas, ni paridad de feeling.

La cobertura Wayland / compositor vive en esta matriz manual.

## Acciones de smoke

Para cada celda, marcar solo si se completaron:

1. Arrancar Baud
2. Escribir ASCII en el shell
3. Redimensionar la ventana
4. Pegar (si el backend de clipboard reporta soporte)
5. Cerrar la ventana limpiamente

Script auxiliar: `tools/linux_session_smoke.sh` (omite sin display; admite `--xvfb`).

## Matriz

Rellenar al menos **dos** entornos antes de declarar feeling Linux como Done.

| Entorno | Backend | Distro / sesión | Arranque | ASCII | Resize | Paste | Cierre | Notas | Quién / fecha |
| ------- | ------- | --------------- | -------- | ----- | ------ | ----- | ------ | ----- | ------------- |
| Wayland compositor (clase wlroots) | Wayland | p. ej. Arch + sesión tiling | | | | | | `force_initial_redraw` vía familia | |
| Ubuntu Wayland | Wayland | Ubuntu GNOME Wayland | | | | | | primary selection a menudo ausente | |
| Ubuntu X11 | X11 | Ubuntu + sesión Xorg | | | | | | | |
| Fedora o Arch | Wayland o X11 | Fedora Workstation / Arch | | | | | | | |
| CI xvfb | X11 only | `ubuntu-latest` + xvfb | n/a | n/a | n/a | n/a | n/a | crash smoke; no Wayland | automatizado |

### Celdas rellenadas (proceso)

| # | Entorno | Resultado | Quién / fecha |
| - | ------- | --------- | ------------- |
| 1 | _(pendiente)_ | | |
| 2 | _(pendiente)_ | | |

## Cómo rellenar

1. Anotar backend real (`display quirks resueltos` en el log al arrancar).
2. Ejecutar las cinco acciones.
3. Si falla algo quirks-related, abrir issue o ampliar la fila en `display_quirks` con evidencia.

## Referencias

1. `src/display_quirks.rs` — tabla y detección.
2. winit 0.30 — `ActiveEventLoopExtWayland` / `ActiveEventLoopExtX11`.
3. Plan de desarrollo — Linux session quirks and distro matrix.
