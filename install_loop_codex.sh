#!/bin/bash
# install_loop_codex.sh — Build and install loop_codex (Agent Loop Codex fork)
#
# Installs alongside standard codex without conflict:
#   - Binary name: loop_codex (not codex)
#   - Same ~/.codex/ config directory (shared, compatible)
#   - Installed to /usr/local/bin/ by default
#
# Usage:
#   ./install_loop_codex.sh          # build release + install
#   ./install_loop_codex.sh --debug  # build debug + install (faster build)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="loop_codex"
BUILD_MODE="release"

# Parse args
if [[ "${1:-}" == "--debug" ]]; then
    BUILD_MODE="debug"
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║           Loop Codex — Install Script                   ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo "║  Binary name:  $BINARY_NAME"
echo "║  Install dir:  $INSTALL_DIR"
echo "║  Build mode:   $BUILD_MODE"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Check prerequisites
if ! command -v cargo &> /dev/null; then
    echo "❌ Error: cargo (Rust toolchain) is not installed."
    echo "   Install it from: https://rustup.rs/"
    exit 1
fi

if ! command -v rustc &> /dev/null; then
    echo "❌ Error: rustc is not installed."
    exit 1
fi

echo "🔧 Rust toolchain: $(rustc --version)"
echo ""

# Build
echo "📦 Building codex-cli ($BUILD_MODE)..."
cd "$REPO_ROOT/codex-rs"

if [[ "$BUILD_MODE" == "release" ]]; then
    cargo build --release -p codex-cli
    BINARY_PATH="$REPO_ROOT/codex-rs/target/release/codex"
else
    cargo build -p codex-cli
    BINARY_PATH="$REPO_ROOT/codex-rs/target/debug/codex"
fi

if [[ ! -f "$BINARY_PATH" ]]; then
    echo "❌ Error: Build succeeded but binary not found at $BINARY_PATH"
    echo "   Checking target directory..."
    find "$REPO_ROOT/codex-rs/target/$BUILD_MODE" -name "codex*" -maxdepth 1 -type f 2>/dev/null || true
    exit 1
fi

echo "✅ Build complete: $BINARY_PATH"
echo ""

# Run tests
echo "🧪 Running tests..."
cd "$REPO_ROOT/codex-rs"
if cargo test -p codex-core --lib -- --test-threads=4 2>&1 | tail -5; then
    echo "✅ Tests passed"
else
    echo "⚠️  Some tests may have failed (check output above)"
    echo "   Continuing with install..."
fi
echo ""

# Install
echo "📥 Installing as '$BINARY_NAME' to $INSTALL_DIR..."

# Check if we need sudo
if [[ -w "$INSTALL_DIR" ]]; then
    cp "$BINARY_PATH" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
else
    echo "   (requires sudo for $INSTALL_DIR)"
    sudo cp "$BINARY_PATH" "$INSTALL_DIR/$BINARY_NAME"
    sudo chmod +x "$INSTALL_DIR/$BINARY_NAME"
fi

echo "✅ Installed: $INSTALL_DIR/$BINARY_NAME"
echo ""

# Verify
if command -v "$BINARY_NAME" &> /dev/null; then
    echo "🎉 Installation complete!"
    echo ""
    echo "   Usage:"
    echo "     $BINARY_NAME --local          # LM Studio (localhost:1234)"
    echo "     $BINARY_NAME --synthetic      # Synthetic API"
    echo "     $BINARY_NAME --openrouter     # OpenRouter API"
    echo "     $BINARY_NAME --oss            # OSS provider selection"
    echo "     $BINARY_NAME --help           # Full help"
    echo ""
    echo "   This is installed alongside regular 'codex' — no conflicts."
else
    echo "⚠️  Binary installed but not found in PATH."
    echo "   Make sure $INSTALL_DIR is in your PATH:"
    echo "     export PATH=\"$INSTALL_DIR:\$PATH\""
fi

# Source provider keys if they exist
KEYS_FILE="$HOME/.codex/provider_keys.env"
if [[ -f "$KEYS_FILE" ]]; then
    echo ""
    echo "💡 Found provider keys at $KEYS_FILE"
    echo "   Add this to your shell profile to auto-load:"
    echo "     source $KEYS_FILE"
fi
