# Install on Windows

Windows support is **experimental**. Baud runs and renders correctly, but the CI gate that verifies ConPTY sessions on Windows still runs as a soft check (it does not block a release on failure), so the label stays experimental until that gate is hardened.

## Requirements

- Windows 10 1809 or later (for ConPTY), or Windows 11.
- A GPU driver that supports DX12 — Baud renders through [wgpu](https://wgpu.rs/).

Both distributed artifacts are statically linked against the MSVC C runtime, so no separate Visual C++ Redistributable install is needed.

## Portable zip

Download `baud-<version>-windows-x64.zip` from the [Releases page](https://github.com/KarloZ7715/Baud/releases), extract it anywhere, and run `baud.exe`. Keep `conpty.dll` and `OpenConsole.exe` next to `baud.exe`; without that pair Baud uses the OS console host instead. No installation or admin rights required.

## MSI installer

Download `baud-<version>-windows-x64.msi` and run it. It installs to `Program Files` (per-machine, admin rights required), adds a Start Menu shortcut, and supports a clean uninstall through Windows' standard "Add or remove programs".

## Unsigned builds

Windows builds are not code-signed yet, so SmartScreen will warn on first run. Choose **More info** then **Run anyway** to continue.

## No self-update on Windows

`baud update` only works on Linux x86_64 installations with an official-install receipt. On Windows, update by downloading the next release from the Releases page and reinstalling — the MSI upgrades the existing install in place.

Continue with [Getting started](../getting-started.md).
