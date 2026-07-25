# About

Baud is a terminal emulator written from scratch in Rust. It renders through the GPU (via [wgpu](https://wgpu.rs/) and [glyphon](https://github.com/grovesNL/glyphon)), parses PTY output with a full VT/ANSI state machine, and runs on Linux and Windows.

## Status

Baud is pre-1.0 software, see the [changelog](changelog.md) for the current version. Linux x86_64 is the primary, actively tested platform. Windows support is experimental: it works, but the runtime CI gate that verifies ConPTY sessions on Windows still runs in soft mode (failures do not block a release), so regressions can slip through before that gate hardens. See [Platform notes](help/platform-notes.md) for the details.

There is no macOS build. The self-update command (`baud update`) and the install script only target Linux x86_64; Windows users update by downloading a new release manually.

## Design goals

- **Fast**: GPU-accelerated rendering and a grid designed around minimizing redraw work.
- **Feature-rich**: tabs and splits, copy mode, incremental search, smart selection, shell integration through OSC 133, clipboard support with an OSC 52 policy, desktop notifications, clickable URLs, and font fallback with ligatures.
- **Cross-platform**: the same codebase targets Linux and Windows; platform-specific code is isolated behind narrow modules (PTY backend, clipboard backend, window integration).

## What Baud does not do (yet)

- No Sixel or kitty graphics protocol support. See the [Terminal API](vt/index.md) page for the full support matrix.
- No macOS build.
- Self-update is Linux-only.

## Getting help

Diagnostics reporting is opt-in and off by default; see [Diagnostics](features/diagnostics.md) for what it collects and how to enable it. For everything else, start at [Troubleshooting](help/troubleshooting.md).
