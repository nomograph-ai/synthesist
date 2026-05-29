# Linux ARM64 Cross-Compilation Guide

## Current Environment
- **Host OS**: macOS (ARM64)
- **Target**: Linux ARM64 (aarch64-unknown-linux-gnu)
- **Project**: nomograph-synthesist (v2.5.1)

## Installed Prerequisites
- ✅ Rust 1.93.0 
- ✅ cross v0.2.5
- ✅ aarch64-unknown-linux-gnu target added
- ✅ aarch64-unknown-linux-musl target added

## Issue
macOS cannot directly execute Linux container images with the current Apple Container Runtime.

## Recommended Solutions

### Option 1: Linux CI/CD Pipeline (RECOMMENDED)
Use GitHub Actions or GitLab CI to build Linux ARM64 automatically:

```yaml
# .github/workflows/linux-arm64.yml
build-linux-arm64:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v3
    - uses: dtolnay/rust-toolchain@stable
      with:
        targets: aarch64-unknown-linux-gnu
    - uses: taiki-e/install-action@cross
    - run: cross build --release --target aarch64-unknown-linux-gnu
    - uses: actions/upload-artifact@v3
      with:
        name: synthesist-linux-arm64
        path: target/aarch64-unknown-linux-gnu/release/synthesist
```

### Option 2: Remote Linux Build
Build on a Linux system (VM, cloud instance, or CI):

```bash
# On any Linux host:
git clone <repo>
cd synthesist
cross build --release --target aarch64-unknown-linux-gnu
# Binary: target/aarch64-unknown-linux-gnu/release/synthesist
```

### Option 3: Docker Compose (on Linux hosts with Docker)
Would work fine on Linux but not on this macOS setup with Apple Containers.

## For Development
- Use native macOS builds: `make build` or `cargo build --release`
- Binary location: `./target/release/synthesist` or `./synthesist`

## References
- cross: https://github.com/cross-rs/cross
- Rust targets: https://doc.rust-lang.org/nightly/rustc/platform-support.html
