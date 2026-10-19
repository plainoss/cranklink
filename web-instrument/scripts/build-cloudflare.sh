#!/usr/bin/env bash
# Build the verified web-instrument dist for Cloudflare Workers Builds.
#
# Workers Builds runs this from web-instrument/ on Ubuntu 24.04 x86_64 with
# Node and Python but no Rust, so the pinned Rust toolchain and wasm-bindgen
# are bootstrapped here. Set SKIP_DEPENDENCY_INSTALL=1 on the build so npm ci
# below is the only dependency install. Works unchanged on a dev machine.
set -euo pipefail

RUST_VERSION=1.90.0
WASM_BINDGEN_VERSION=0.2.129
WASM_BINDGEN_LINUX_SHA256=82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e

web="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$web/.." && pwd)"
tools="${CRANKLINK_TOOLS_DIR:-$HOME/.cache/cranklink-build}"

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
fi
export PATH="$HOME/.cargo/bin:$PATH"
rustup toolchain install "$RUST_VERSION" --profile minimal --target wasm32-unknown-unknown

if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $WASM_BINDGEN_VERSION" ]]; then
  if [[ "$(uname -sm)" != "Linux x86_64" ]]; then
    echo "Install wasm-bindgen $WASM_BINDGEN_VERSION (cargo install wasm-bindgen-cli --version $WASM_BINDGEN_VERSION --locked)" >&2
    exit 1
  fi
  name="wasm-bindgen-$WASM_BINDGEN_VERSION-x86_64-unknown-linux-musl"
  mkdir -p "$tools"
  curl --proto '=https' -sSfL -o "$tools/$name.tar.gz" \
    "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$WASM_BINDGEN_VERSION/$name.tar.gz"
  echo "$WASM_BINDGEN_LINUX_SHA256  $tools/$name.tar.gz" | sha256sum -c -
  tar -xzf "$tools/$name.tar.gz" -C "$tools"
  export PATH="$tools/$name:$PATH"
fi

cd "$repo"
# Remap build-machine paths so they never reach the public WASM.
RUSTFLAGS="--remap-path-prefix=$repo=. --remap-path-prefix=$HOME=/build-home" \
  cargo "+$RUST_VERSION" build --release --locked --target wasm32-unknown-unknown --lib
rm -rf "$web/pkg"
mkdir "$web/pkg"
wasm-bindgen target/wasm32-unknown-unknown/release/cranklink.wasm --out-dir "$web/pkg" --target web

cd "$web"
npm ci
npm test
npm run build
python3 "$repo/.github/scripts/check-web-assets.py" dist
