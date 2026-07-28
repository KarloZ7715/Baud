# Building

## Toolchain

Baud uses the 2021 edition and pins MSRV `rust-version = "1.87.0"` in `Cargo.toml`. Install a matching toolchain with [rustup](https://rustup.rs/).

```sh
git clone https://github.com/KarloZ7715/Baud.git
cd Baud
cargo build --release
```

The binary is `target/release/baud`. For a faster edit loop, `cargo run` (debug) is enough.

Also see [Build from source](../install/source.md) for the same dependency list in the install section.

## Linux system dependencies

Baud links X11, Wayland, and Vulkan client libraries through winit and wgpu. On Debian/Ubuntu:

```sh
sudo apt-get install -y \
  pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev \
  libxi-dev libxrandr-dev libvulkan-dev libfontconfig1-dev
```

CI release jobs install the same set (`.github/workflows/release.yml`). Other distributions need the equivalent `-dev`/`-devel` packages.

## Windows

Use the MSVC toolchain from rustup and Visual Studio Build Tools with the "Desktop development with C++" workload. No extra system libraries beyond that are required to compile. Runtime expectation is Windows 10 1809+ with a DX12-capable driver. Release artifacts set `RUSTFLAGS=-C target-feature=+crt-static` so they do not need a separate VC++ redistributable; local debug builds use the default dynamic CRT.

macOS is not supported.

## Logging

Defaults live in `src/diagnostics/logging.rs`, not in `main.rs`.

| Order | Source |
| --- | --- |
| 1 | `RUST_LOG` via `EnvFilter::try_from_default_env` |
| 2 | else `diagnostics.log_level` in config → `baud={level},wgpu_core=warn,winit=warn` |
| 3 | else `baud=warn,wgpu_core=warn,winit=warn` |

If `RUST_LOG` is set, config level changes do not override it.

```sh
RUST_LOG=baud=debug,wgpu_core=warn,winit=warn cargo run
```

On Linux, `LIBGL_ALWAYS_SOFTWARE=1` forces software rendering when you are isolating GPU issues.

## Git hooks

Install [lefthook](https://lefthook.dev/) once after clone:

```sh
lefthook install
```

Hooks run fmt/clippy on commit, full tests on push, and Conventional Commits on `commit-msg`. Details are in [Conventions](conventions.md).
