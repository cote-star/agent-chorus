#!/bin/bash
set -euo pipefail

# Record the Agent Chorus demo as an animated WebP.
#
# Prerequisites:
#   - Node.js >= 18
#   - img2webp (brew install webp)
#
# Usage:
#   bash scripts/record_demo.sh
#   bash scripts/record_demo.sh --input fixtures/demo/player.html --output docs/demo.webp
#   bash scripts/record_demo.sh --input fixtures/demo/player-skill-setup.html --output docs/demo-skill.webp

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Ensure puppeteer is installed
if ! node -e "require('puppeteer')" 2>/dev/null; then
    echo "Installing puppeteer..."
    cd "$ROOT_DIR" && npm install --save-dev puppeteer
fi

# Check for img2webp
if ! command -v img2webp &>/dev/null; then
    echo "Error: img2webp is required. Install with: brew install webp"
    exit 1
fi

echo "Recording demo..."
node "$SCRIPT_DIR/record_demo.js" "$@"
