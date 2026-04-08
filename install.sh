#!/bin/bash
# Volta installer — installs the compiler and standard library

set -e

echo "Installing Volta..."

# Install the compiler
cargo install --path . --quiet

# Install stdlib to ~/.volta/lib/
VOLTA_HOME="$HOME/.volta"
mkdir -p "$VOLTA_HOME/lib"

# Copy stdlib files
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ -d "$SCRIPT_DIR/lib" ]; then
    cp "$SCRIPT_DIR/lib/"*.vlt "$VOLTA_HOME/lib/"
    echo "✓ Installed stdlib to $VOLTA_HOME/lib/"
else
    echo "warning: no lib/ folder found, skipping stdlib"
fi

echo "✓ Volta installed successfully"
echo ""
echo "Try it:"
echo "  volta new myproject"
echo "  cd myproject"
echo "  volta main.vlt"
