# Platform backends

Each session talks to a shell through a `SessionBackend` and a `WakeSource`. The contract is OS-agnostic; the implementations are not.

## Contract

`src/pty/contract.rs` defines:

- **`SessionBackend`** — spawn the child, write input, resize, interrupt, graceful shutdown, force kill, read output, non-blocking mode.
- **`WakeSource`** — wake a blocked PTY wait when the GUI enqueues a command.

`src/pty/channel.rs` wraps commands (`Input`, `Resize`, `Interrupt`, `Shutdown`). Every send also calls `wakeup.wake()` so the PTY thread does not sit in a blocking read while work is pending.

## Unix

`src/pty/unix.rs` opens a PTY master/slave pair, uses an eventfd-style wake source, applies winsize via ioctl, and signals the child on shutdown. This is the default backend on Linux.

## Windows ConPTY

`src/pty/windows.rs` uses the ConPTY API (`CreatePseudoConsole`, pipes, `CreateProcessW` with the pseudo-console attribute). `ConPtyWake` plus pipe peeking implements `WakeSource` and readiness waits. Session conformance tests exercise this path in CI (soft gate today).

## WSL profile

`src/pty/wsl.rs` is not a third PTY implementation. On Windows it launches `wsl.exe` under ConPTY. The executable path is resolved with `GetSystemDirectoryW` + `wsl.exe` so a malicious `wsl.exe` earlier on `PATH` cannot be preferred. Distro, directory, user, and command flags are assembled in `build_wsl_argv`.

## Display quirks

`src/display_quirks.rs` classifies the winit backend (Wayland, X11, other) and, when possible, the compositor family from the environment (Hyprland, wlroots, GNOME, KDE, and others). Quirks include forcing an initial redraw, cursor-left behavior, and whether primary selection is likely available. These flags adjust window behavior; they do not replace the PTY backends.

## Where to change this

| Change | Start in |
| --- | --- |
| Backend trait or command set | `src/pty/contract.rs`, `channel.rs` |
| Linux PTY bugs | `src/pty/unix.rs` |
| ConPTY / Windows session bugs | `src/pty/windows.rs` |
| WSL launch or PATH hardening | `src/pty/wsl.rs` |
| Compositor-specific window behavior | `src/display_quirks.rs` |
