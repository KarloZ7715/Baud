# Baud

A terminal emulator written from scratch in Rust. GPU-accelerated rendering, PTY support, and ANSI/VT parsing - built to be fast and minimal.

---

## Documentacion

*(in development)*

## Experimental Installation (Linux x86_64)

After the first release is published, install the latest Linux x86_64 binary with:

```sh
curl -fsSL https://raw.githubusercontent.com/KarloZ7715/Baud/master/install.sh | sh
```

Baud is pre-1.0 software. Windows, macOS, and other architectures are not available through this installer yet.

## Experimental Installation (Windows x64)

After the first release is published, Windows x64 users can choose either:

- **Portable**: download `baud-<version>-windows-x64.zip` from the [Releases page](https://github.com/KarloZ7715/Baud/releases), extract it anywhere, and run `baud.exe`. No installation or admin rights required.
- **MSI installer**: download `baud-<version>-windows-x64.msi` and run it. Installs to `Program Files`, adds a Start Menu shortcut, and supports clean uninstall. Requires admin rights (per-machine install).

Requirements: Windows 10 1809+ (for ConPTY) or Windows 11, with a DX12-capable GPU driver (Baud renders through wgpu). Both artifacts are statically linked against the MSVC C runtime, so no separate VC++ Redistributable install is needed.

Windows support is **experimental**: builds are unsigned, so Windows SmartScreen will warn on first run (`More info` -> `Run anyway`). Code signing and a "supported" label are tracked as follow-up work once Windows runtime CI is consistently green.

## License

Baud is released under the [Apache License, Version 2.0](LICENSE).

---
