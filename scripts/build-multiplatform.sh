#!/bin/bash
# Multi-platform build script for synthesist
# Builds native macOS and cross-compilation targets

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "Synthesist Multi-Platform Build Script"
echo "========================================"
echo "Project: $PROJECT_ROOT"
echo ""

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to run a build
run_build() {
    local target=$1
    local tool=$2
    local desc=$3
    
    echo -e "${YELLOW}Building for: $desc ($target)${NC}"
    
    if [ "$tool" = "cargo" ]; then
        cargo build --release --target "$target" || {
            echo -e "${RED}Failed to build for $target${NC}"
            return 1
        }
        echo -e "${GREEN}✓ Built successfully: target/$target/release/synthesist${NC}"
    elif [ "$tool" = "cross" ]; then
        if ! command_exists cross; then
            echo -e "${RED}cross not installed. Install with: cargo install cross${NC}"
            return 1
        fi
        cross build --release --target "$target" || {
            echo -e "${RED}Failed to cross-compile for $target${NC}"
            return 1
        }
        echo -e "${GREEN}✓ Cross-compiled successfully: target/$target/release/synthesist${NC}"
    fi
}

# Detect host platform
UNAME_S=$(uname -s)
UNAME_M=$(uname -m)

case "$UNAME_S" in
    Darwin)
        HOST_OS="macOS"
        if [ "$UNAME_M" = "arm64" ]; then
            HOST_ARCH="ARM64"
            NATIVE_TARGET="aarch64-apple-darwin"
        else
            HOST_ARCH="x86_64"
            NATIVE_TARGET="x86_64-apple-darwin"
        fi
        ;;
    Linux)
        HOST_OS="Linux"
        if [ "$UNAME_M" = "aarch64" ]; then
            HOST_ARCH="ARM64"
            NATIVE_TARGET="aarch64-unknown-linux-gnu"
        else
            HOST_ARCH="x86_64"
            NATIVE_TARGET="x86_64-unknown-linux-gnu"
        fi
        ;;
    *)
        echo -e "${RED}Unsupported OS: $UNAME_S${NC}"
        exit 1
        ;;
esac

echo "Host Platform: $HOST_OS ($HOST_ARCH)"
echo "Native Target: $NATIVE_TARGET"
echo ""

# Parse arguments
BUILD_NATIVE=true
BUILD_LINUX_ARM64=false
BUILD_LINUX_X86_64=false
BUILD_MACOS_UNIVERSAL=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --all)
            BUILD_LINUX_ARM64=true
            BUILD_LINUX_X86_64=true
            BUILD_MACOS_UNIVERSAL=true
            shift
            ;;
        --linux-arm64)
            BUILD_LINUX_ARM64=true
            shift
            ;;
        --linux-x86_64)
            BUILD_LINUX_X86_64=true
            shift
            ;;
        --macos-universal)
            BUILD_MACOS_UNIVERSAL=true
            shift
            ;;
        --skip-native)
            BUILD_NATIVE=false
            shift
            ;;
        --help)
            cat << EOF
Usage: $0 [OPTIONS]

Options:
  --all                Build all targets (requires cross/multiplatform tooling)
  --linux-arm64        Cross-compile for Linux ARM64
  --linux-x86_64       Cross-compile for Linux x86_64
  --macos-universal    Build universal macOS binary (macOS only)
  --skip-native        Skip native platform build
  --help              Show this help message

Examples:
  # Native build only (default)
  $0

  # Cross-compile to Linux ARM64
  $0 --linux-arm64

  # Build everything
  $0 --all

EOF
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

BUILD_FAILED=0
BUILD_SUCCESS=0

# Build native target
if [ "$BUILD_NATIVE" = true ]; then
    echo "=== Native Build ==="
    if run_build "$NATIVE_TARGET" "cargo" "Native ($HOST_OS $HOST_ARCH)"; then
        ((BUILD_SUCCESS++))
    else
        ((BUILD_FAILED++))
    fi
    echo ""
fi

# Build Linux ARM64 (cross-compilation)
if [ "$BUILD_LINUX_ARM64" = true ]; then
    echo "=== Linux ARM64 Cross-Compilation ==="
    if [ "$HOST_OS" = "Linux" ] && [ "$HOST_ARCH" = "ARM64" ]; then
        echo "Host is native Linux ARM64, using cargo..."
        if run_build "aarch64-unknown-linux-gnu" "cargo" "Linux ARM64"; then
            ((BUILD_SUCCESS++))
        else
            ((BUILD_FAILED++))
        fi
    else
        echo "Host is $HOST_OS, using cross..."
        if run_build "aarch64-unknown-linux-gnu" "cross" "Linux ARM64"; then
            ((BUILD_SUCCESS++))
        else
            echo -e "${YELLOW}Note: To build Linux binaries from macOS, use GitHub Actions or build on a Linux system.${NC}"
            echo -e "${YELLOW}See .cross/linux-arm64-build.md for more details.${NC}"
            ((BUILD_FAILED++))
        fi
    fi
    echo ""
fi

# Build Linux x86_64 (cross-compilation)
if [ "$BUILD_LINUX_X86_64" = true ]; then
    echo "=== Linux x86_64 Cross-Compilation ==="
    if [ "$HOST_OS" = "Linux" ] && [ "$HOST_ARCH" = "x86_64" ]; then
        if run_build "x86_64-unknown-linux-gnu" "cargo" "Linux x86_64"; then
            ((BUILD_SUCCESS++))
        else
            ((BUILD_FAILED++))
        fi
    else
        if run_build "x86_64-unknown-linux-gnu" "cross" "Linux x86_64"; then
            ((BUILD_SUCCESS++))
        else
            ((BUILD_FAILED++))
        fi
    fi
    echo ""
fi

# Build universal macOS (macOS only)
if [ "$BUILD_MACOS_UNIVERSAL" = true ]; then
    if [ "$HOST_OS" != "macOS" ]; then
        echo -e "${RED}macOS universal build only supported on macOS${NC}"
        ((BUILD_FAILED++))
    else
        echo "=== macOS Universal Binary ==="
        echo "Building for both x86_64 and ARM64..."
        if run_build "x86_64-apple-darwin" "cargo" "macOS x86_64" && \
           run_build "aarch64-apple-darwin" "cargo" "macOS ARM64"; then
            echo -e "${YELLOW}Creating universal binary...${NC}"
            if lipo -create \
                target/x86_64-apple-darwin/release/synthesist \
                target/aarch64-apple-darwin/release/synthesist \
                -output synthesist-universal; then
                echo -e "${GREEN}✓ Universal binary created: synthesist-universal${NC}"
                ((BUILD_SUCCESS++))
            else
                echo -e "${RED}Failed to create universal binary${NC}"
                ((BUILD_FAILED++))
            fi
        else
            ((BUILD_FAILED++))
        fi
    fi
    echo ""
fi

# Summary
echo "========================================"
echo "Build Summary"
echo "========================================"
echo -e "${GREEN}Successful: $BUILD_SUCCESS${NC}"
echo -e "${RED}Failed: $BUILD_FAILED${NC}"
echo ""

if [ $BUILD_FAILED -eq 0 ]; then
    echo -e "${GREEN}All builds completed successfully!${NC}"
    exit 0
else
    echo -e "${RED}Some builds failed.${NC}"
    exit 1
fi
