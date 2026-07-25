# Build from source

Baud targets the 2021 Rust edition and requires Rust `1.87.0` or newer (the pinned MSRV in `Cargo.toml`). Install it with [rustup](https://rustup.rs/) if you do not have a toolchain already.

```sh
git clone https://github.com/KarloZ7715/Baud.git
cd Baud
cargo build --release
```

The binary is written to `target/release/baud`.

## Linux system dependencies

Baud links against X11, Wayland, and Vulkan client libraries through `winit` and `wgpu`. On Debian/Ubuntu, install:

```sh
sudo apt-get install pkg-config libxkbcommon-dev libwayland-dev libx11-dev \
  libxcursor-dev libxi-dev libxrandr-dev libvulkan-dev libfontconfig1-dev
```

Package names differ on other distributions; look for the `-dev`/`-devel` equivalents of the same libraries.

## Windows

Building on Windows needs the standard Rust MSVC toolchain (`rustup` installs it by default) plus the Visual Studio Build Tools with the "Desktop development with C++" workload, which provides the MSVC linker. No additional system libraries are required beyond that.

## Running

```sh
cargo run --release
```

Continue with [Getting started](../getting-started.md) once the build succeeds.
