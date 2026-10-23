#!/usr/bin/env bash
set -euo pipefail

# cranklink fast verification script
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> [1/6] Checking Rust formatting..."
# Ask cargo: rustup installs a cargo-fmt proxy even without rustfmt.
if cargo fmt --version >/dev/null 2>&1; then
  cargo fmt --check || { echo "Formatting failed. Run 'cargo fmt' to fix."; exit 1; }
else
  echo "    Skipped: rustfmt is not installed (rustup component add rustfmt)."
fi

echo "==> [2/6] Type-checking the USB-only build (tests cover the default BLE build)..."
cargo check --locked --no-default-features --lib --bin cranklink

echo "==> [3/6] Checking the WebAssembly build (browser-only modules)..."
if rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  cargo check --locked --target wasm32-unknown-unknown --lib
else
  echo "    Skipped: run 'rustup target add wasm32-unknown-unknown' to check src/wasm.rs and src/ble/browser.rs."
fi

echo "==> [4/6] Running core Rust unit tests..."
cargo test --locked --lib --bin cranklink

echo "==> [5/6] Running firmware simulator and patcher tests..."
PYTHON="${CRANKLINK_SIM_PYTHON:-python3}"
if "$PYTHON" -c "import unicorn" >/dev/null 2>&1; then
  "$PYTHON" -m unittest discover -s tools/firmware-sim -p 'test_*.py'
else
  echo "    Skipped: install tools/firmware-sim/requirements-test.txt (or set CRANKLINK_SIM_PYTHON to a venv python)."
fi

if [ -d "web-instrument" ]; then
  echo "==> [6/6] Verifying Web Instrument (types + unit tests)..."
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
