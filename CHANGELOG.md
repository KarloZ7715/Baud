# Changelog

All user-facing changes are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Baud follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.8](https://github.com/KarloZ7715/Baud/compare/v0.0.7...v0.0.8) - 2026-07-23

### Added

- *(mouse)* add focus event reporting and truthful DECRQM for mouse modes
- *(cli)* parse launch flags into LaunchOptions
- *(launch)* wire LaunchOptions into ProcessConfig, window and hold
- *(desktop)* advertise terminal launch flags in desktop entry
- *(input)* add keyboard selection extension and paste-primary bind
- *(release)* build and upload Windows zip and MSI
- *(config)* default minimum_contrast to 1.0 with clamping
- *(config)* add session kind, distro and wsl_cwd to ProcessSection
- *(wsl)* add WSL cmdline builder and System32 resolver
- *(windows)* wire WSL profile into ConPTY spawn and set session title
- *(renderer)* OS-aware font fallbacks and locale detection
- *(windows)* apply Mica backdrop when window opacity < 1.0
- *(input)* add Windows dual binding for theme-picker

### Fixed

- *(renderer)* force full damage on scrollback offset change
- *(window)* mark term dirty on every send_input
- *(renderer)* track cursor position for incremental damage
- *(window)* throttle GUI-originated selection redraws
- *(renderer)* trim glyphon atlas after every present
- *(mouse)* handle multi-mode DECSET and correct X10 release encoding
- *(mouse)* focus pane before forwarding in splits and reject out-of-pane events
- *(renderer)* run selection/cursor damage diffing before cache guard
- *(grid)* mark full damage after scroll-down region
- *(cli)* keep -e only and set app_id on both Wayland and X11
- *(input)* prepend ESC for Alt+Enter in classic key encoding
- *(input)* normalize shifted bracket/plus symbols before lookup
- *(startup)* gate time-to-first-frame log on a real paint
- *(contrast)* optimize adjust function for minimum ratio check
- *(window)* match theme-picker chord by physical key on Windows
- *(windows)* stop panic on translucent swapchain configure
- *(renderer)* prioritize monospace fonts in Windows fallback order
- *(config)* resolve Windows default font to Cascadia Mono
- *(renderer)* clamp row index to live grid and isolate render panics

### Packaging

- *(packaging)* add Windows portable zip build script
- *(packaging)* add WiX v4 MSI installer for Windows

### Performance

- *(renderer)* persist contrast cache across frames
- *(startup)* instrument cold-start phases with timing logs
- *(startup)* parallelize font scan with GPU negotiation
- *(startup)* paint theme background before fonts finish

## [0.0.7](https://github.com/KarloZ7715/Baud/compare/v0.0.6...v0.0.7) - 2026-07-13

### Added

- *(diagnostics)* añadir config diagnostics/reporting + consent state machine + persist toml
- *(diagnostics)* embeber DSN por defecto del proyecto en el binario
- *(install)* registrar desktop entry e íconos XDG desde el tarball verificado
- *(cli)* add command dispatcher for version, help and update
- *(install)* write and validate official-install ownership receipt
- *(updater)* implement verified self-update with signed manifest

### Fixed

- *(release)* publicar borrador sin checkout
- *(diagnostics)* traducir texto user-facing a inglés y corregir bugs de review
- *(diagnostics)* corregir formato de timestamp a ISO 8601 requerido por Sentry
- *(install)* eliminar código muerto y usar awk para Exp y evitar bug con sed
- *(ci)* restaurar jobs originales y agregar shell-fixtures sin borrar nada
- *(ci)* instalar desktop-file-utils antes de validar desktop entry
- *(updater)* verifica manifiesto y limpia staging
- *(updater)* desacopla version instalada en tests

### Packaging

- *(packaging)* incluir desktop entry e íconos en el tarball de release
- *(packaging)* crea target para signer

## [0.0.6] - 2026-07-12

This is the first experimental Baud release for Linux x86_64.

### Packaging

- Publish AppImage, deb, rpm, and tarball assets with a SHA-256 manifest.
- Provide a checksum-verified installer for the Linux x86_64 tarball.

### Added

- Distribute Baud through normal GitHub Releases while it remains pre-1.0 software.

### Compatibility

- Windows and macOS are not supported platforms yet.
