# Linux ARM64 Build Setup - Session Summary

**Session ID**: `linux-arm64-build`
**Phase**: PLAN
**Date**: 2026-05-29 (adapted for v3 workspace 2026-06)

## Objective
Enable Linux ARM64 (aarch64-unknown-linux-gnu) builds for synthesist
(nomograph-synthesist v3.0.0-pre.1), a member of the nomograph 2-crate
workspace (claim + synthesist) over gamma/redb.

## v3 Substrate Note
synthesist v3 dropped Oxigraph/RocksDB in favor of gamma/redb. There is no
C++/librocksdb dependency in the graph, so cross-compilation no longer needs
a C++ toolchain, bindgen, or any librocksdb-sys build-script workarounds. A
plain `cargo build --release --target aarch64-unknown-linux-gnu` produces the
binary. Builds are scoped to the synthesist binary
(`-p nomograph-synthesist --bin synthesist`) so they behave the same whether
invoked from the synthesist crate dir or the workspace root.

## Current Status

### Completed
1. **Environment Setup**
   - Rust 1.93.0 toolchain
   - cross v0.2.5 installed (optional for v3; no longer required)
   - aarch64-unknown-linux-gnu target added

2. **Native Build Verification**
   - macOS ARM64 native build works
   - `make build` produces `./target/release/synthesist` (macOS binary)

3. **Documentation & Tools**
   - `.cross/linux-arm64-build.md` - cross-compilation guide (v3)
   - `scripts/build-multiplatform.sh` - multi-platform build script (v3)
   - Platform detection and automatic target selection

### Limitations
- macOS cannot directly execute Linux ELF binaries
- Direct cross-compilation on macOS -> Linux requires either a target linker
  (e.g. cargo-zigbuild) or a Linux host / CI

## Available Build Options

### Option 1: GitLab CI Pipeline (CANONICAL)
The `tool-rust@v4.2.3` pipeline component already ships a `build:linux-arm64`
job that runs `cargo zigbuild --release --target aarch64-unknown-linux-gnu`
on a Linux runner and uploads `synthesist-linux-arm64`. It is part of the
standard build matrix alongside `build:darwin-arm64`; no extra CI config is
needed beyond the component include in `.gitlab-ci.yml`.

### Option 2: Native Linux ARM64 Build
**Best for**: development on ARM64 hardware (Graviton, Raspberry Pi, etc.)

```bash
cargo build --release -p nomograph-synthesist --bin synthesist
# Binary at: target/release/synthesist
```

### Option 3: Cross-compile from macOS
```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release -p nomograph-synthesist --bin synthesist \
  --target aarch64-unknown-linux-gnu
# Binary at: target/aarch64-unknown-linux-gnu/release/synthesist
```
With no RocksDB/C++ in the graph, only a target linker is needed; `cross` is
no longer required (use `cargo zigbuild` for the simplest setup).

## Tools

### 1. Multi-Platform Build Script
**Location**: `scripts/build-multiplatform.sh`

```bash
# Native macOS build
./scripts/build-multiplatform.sh

# Cross-compile to Linux ARM64
./scripts/build-multiplatform.sh --linux-arm64

# Build everything
./scripts/build-multiplatform.sh --all

# macOS universal binary
./scripts/build-multiplatform.sh --macos-universal
```

### 2. Cross-Compilation Guide
**Location**: `.cross/linux-arm64-build.md`

## Key Facts
- All Rust tooling installed
- No RocksDB/Oxigraph/C++ in v3; cross-compilation is plain Rust
- Native macOS builds working
- Canonical Linux ARM64 build is the CI `build:linux-arm64` job

## Files
- `scripts/build-multiplatform.sh` (brought from origin/main, adapted to v3)
- `.cross/linux-arm64-build.md` (brought from origin/main, adapted to v3)
- `BUILD-LINUX-ARM64.md` (this file)
- `mise.toml` (brought from origin/main)

## References
- cross: https://github.com/cross-rs/cross
- cargo-zigbuild: https://github.com/rust-cross/cargo-zigbuild
- Rust platform support: https://doc.rust-lang.org/nightly/rustc/platform-support.html
