# Changelog

All user-facing changes are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Baud follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Full documentation is published at <https://karloz7715.github.io/Baud/>.

## [Unreleased]

## [0.0.9](https://github.com/KarloZ7715/Baud/compare/v0.0.8...v0.0.9) - 2026-08-05

### Added

- *(windows)* GUI subsystem, parent console attach and file log
- *(diagnostics)* reload log level and route panics to the log file
- *(notifications)* send desktop notifications via notify-rust
- *(config)* add render.vsync option
- *(config)* add DecorationsKind enum with backward-compatible bool parsing
- *(window,renderer)* implement custom title bar, DPI scaling and interactions
- *(ansi)* add PackedColor for compact Cell storage
- *(ansi)* add AttrFlags, PackedAttrs and underline color table
- *(diagnostics)* add latency probe for key-to-present
- *(config)* auto-detect monitor refresh for max_fps
- *(diagnostics)* log surface format at startup
- *(renderer)* add font.text_contrast mask curve
- *(themes)* add light variant for each preset family
- *(theme)* raise default minimum_contrast to 1.5
- *(config)* add light/dark theme model with mode, dark, light
- *(config)* resolve system color scheme via winit and xdg portal
- *(theme-picker)* group presets by polarity and write to dark/light
- *(renderer)* draw window buttons as vector masks
- *(renderer)* fuse active tab with grid in variant C
- *(renderer)* match active tab opacity with grid
- *(tab-bar)* resolve tab title from cwd and process
- *(tab-bar)* add activity dot for background sessions
- *(tab-bar)* detect foreground process for tab titles on unix
- *(tab-bar)* show process icon in tab titles when the font supports it
- *(tab-bar)* animate tab and window button hover backgrounds
- *(tab-bar)* merge foreground process titles, icons and hover fades
- *(renderer)* outline unfocused cursor and merge decoration line runs
- *(renderer)* expand process icon map for tab titles
- *(ansi)* report the terminfo capabilities Baud actually has
- *(ansi)* answer the OSC 1337 capabilities query
- *(ansi)* give Term the session default colors
- *(ansi)* accept colon-separated extended color parameters
- *(ansi)* always answer OSC 10/11/12 color queries
- *(ansi)* always answer OSC 4 queries and accept color pairs
- *(ansi)* implement OSC 104/110/111/112 color resets

### Fixed

- *(windows)* embed app icon in exe and fix MSI external cab dependency
- *(windows)* open URLs via ShellExecuteW
- *(windows)* initialize COM before showing toast notifications
- *(windows)* gate COM init helper on cfg(not(test)) too
- *(renderer)* widen interned family id to u32
- *(updater)* allow share/man/man1 in release archive validation
- *(window)* clear stale tab hover when moving to title-bar buttons or resize border
- *(renderer)* use row count in ensure_rows
- *(event_loop)* recover poisoned term mutex in drain
- *(renderer)* catch wgpu configure panic in resize
- *(renderer)* prefer non-sRGB 8-bit surface format
- *(renderer)* linearize clear color on sRGB targets
- *(renderer)* pick glyph color mode by surface
- *(themes)* restore embedded presets to upstream fidelity
- *(renderer)* decouple chrome text contrast from user setting
- *(tab-bar)* contain tab chrome and close scrub within the bar
- *(renderer)* keep pane dirty when the frame came from cache
- *(window)* clear pane dirty on present, not on redraw request
- *(renderer)* repair curly underline and DPI-aware decoration metrics
- *(renderer)* degrade instead of panicking on misaligned rows
- *(window)* recover from a poisoned term mutex instead of panicking
- *(pty)* treat EIO on the master as end of session
- *(watchdog)* do not report a sleeping event loop as stalled
- *(renderer)* pass window_focused arg in misaligned-rows test
- *(ansi)* parse X11 color specs with xterm coverage
- *(window)* apply reloaded config to every session
- *(ansi)* let SGR 22 cancel faint as well as bold
- *(ansi)* restore color overrides when a TUI leaves alt screen
- *(pty)* serialize shell env tests to avoid a data race

### Performance

- *(renderer)* gate glyph reset on metrics change
- *(window)* skip resize grid dump unless debug on
- *(window)* tune wgpu device and surface for latency
- *(window)* avoid blocking term lock in send_input
- *(renderer)* make GlyphKey Copy via string interning
- *(windows)* replace ConPTY polling with overlapped I/O
- *(grapheme)* add ASCII/Latin-1 fast path
- *(event_loop)* grow PTY read buffer to 64KiB
- *(renderer)* make is_wide_continuation O(1)
- *(renderer)* resolve family id once per build
- *(renderer)* write text glyphs into caller buffer
- *(renderer)* index glyph cache by dense id
- *(renderer)* store display list per row
- *(renderer)* cache custom glyphs per row
- *(grid)* drop per-frame clone in GridDamage::take
- *(grid)* recycle row buffers on scroll to avoid allocations
- *(grid)* O(1) full-screen scroll via VecDeque
- *(renderer)* reuse row cache instead of full rebuild on scroll
- *(grid)* shrink Cell from 44 to 24 bytes
- *(renderer)* cache ligature shaping and glyph key
- *(renderer)* incremental scrollback scroll damage
- *(event_loop)* add time budget to drain pass
- *(event_loop)* let keyboard echo skip the max_fps throttle
- *(event_loop)* yield after a drain pass with pending backlog

## [0.0.8](https://github.com/KarloZ7715/Baud/compare/v0.0.7...v0.0.8) - 2026-07-25

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
- *(config)* make defaults introspectable for docs generation
- *(docs)* generate config, keybindings, themes and CLI reference
- *(docs)* add example config, man page and stable anchors

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
- *(docs-gen)* escape pipe in chord table cells
- *(github)* use standard documentation label in issue template
- *(input)* log invalid keybinding warning in English
- *(docs)* update link check configuration to exclude specific path
- *(docs)* refine link check exclusion pattern for 404 handling

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
