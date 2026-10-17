#!/usr/bin/env bash
set -euo pipefail

# cranklink fast verification script
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> [1/4] Checking Rust formatting..."
if command -v cargo-fmt >/dev/null 2>&1 || cargo fmt --version >/dev/null 2>&1; then
  cargo fmt --check || { echo "Formatting failed. Run 'cargo fmt' to fix."; exit 1; }
fi

echo "==> [2/4] Running Rust type & borrow checking (BLE + Core)..."
cargo check --locked --features ble --bin cranklink

echo "==> [3/4] Running core Rust unit tests..."
cargo test --locked --lib --bin cranklink

if [ -d "web-instrument" ]; then
  echo "==> [4/4] Verifying Web Instrument (types + unit tests)..."
  cd web-instrument
  if [ ! -d "node_modules" ]; then
    echo "    Missing node_modules; installing with npm ci..."
    npm ci
  fi
  npm run typecheck
  npm test
fi

echo ""
echo " All local checks passed successfully!"
