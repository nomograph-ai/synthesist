# Linux ARM64 Cross-Compilation Guide

## Current Environment
- **Host OS**: macOS (ARM64)
- **Target**: Linux ARM64 (aarch64-unknown-linux-gnu)
- **Project**: nomograph-synthesist (v3.0.0-pre.1), member of the nomograph
  2-crate workspace (claim + synthesist) over gamma/redb

## v3 Substrate Note
synthesist v3 runs on the gamma/redb substrate. There is no Oxigraph and no
RocksDB, so there is no C++/librocksdb dependency to cross-compile. That
removes the historical cross-compilation pain: no bindgen, no aarch64 C++
toolchain, no librocksdb-sys build script to coax. A plain
`cargo build --release --target aarch64-unknown-linux-gnu` is sufficient.

## Installed Prerequisites
- Rust 1.93.0
- cross v0.2.5 (optional; only for container-based builds from macOS)
- aarch64-unknown-linux-gnu target added

## Issue
macOS cannot directly execute Linux container images with the current Apple
Container Runtime, so the canonical Linux ARM64 build runs in CI.

## Recommended Solutions

### Option 1: GitLab CI Pipeline (CANONICAL)
The `tool-rust@v4.2.3` pipeline component already provides a
`build:linux-arm64` job that runs `cargo zigbuild --release --target
aarch64-unknown-linux-gnu` on a Linux runner and uploads the
`synthesist-linux-arm64` artifact. No extra config is needed beyond the
component include in `.gitlab-ci.yml`.

### Option 2: Native Linux ARM64 Build
Build on a Linux ARM64 host (Graviton, Raspberry Pi, etc.):

```bash
git clone <repo>
cd synthesist
cargo build --release -p nomograph-synthesist --bin synthesist
# Binary: target/release/synthesist
```

### Option 3: Cross-compile from macOS
```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release -p nomograph-synthesist --bin synthesist \
  --target aarch64-unknown-linux-gnu
# Binary: target/aarch64-unknown-linux-gnu/release/synthesist
```
With no RocksDB/C++ in the dependency graph this needs only a linker for the
target (or `cargo zigbuild`); `cross` is no longer required.

## For Development
- Native macOS builds: `make build` or `cargo build --release`
- Binary location: `./target/release/synthesist`

## References
- cross: https://github.com/cross-rs/cross
- cargo-zigbuild: https://github.com/rust-cross/cargo-zigbuild
- Rust targets: https://doc.rust-lang.org/nightly/rustc/platform-support.html
