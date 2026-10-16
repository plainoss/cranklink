# Browser instrument

Open [cranklink on GitHub Pages](https://plainoss.github.io/cranklink/). The browser instrument uses the same Rust core as the native CLI, compiled to WASM. It provides component discovery and motor identity, firmware inspection, schema-supported settings reads/writes, serial-bound backups and restore, plus a command terminal for ordered read/set/restore batches. The responsive layout supports phone, tablet, and desktop screens, with navigation and staged-change review adapted to smaller displays.

Use a browser with the required WebUSB, Web Serial, or Web Bluetooth API, such as Chrome or Edge where supported. Browser and operating-system support determine which transports work; a phone supporting one API may lack another. Unavailable device buttons are disabled with an explanation; the simulator remains available to explore settings without hardware. A compatible BLE gateway is required for wireless access. The first visit may reload once while a service worker enables the isolation needed by WASM. Allow site storage and service workers.

Firmware is not included. Wired flashing/recovery retains the CLI's model, board, serial, and companion checks. BLE settings and opt-in firmware relay are experimental; BLE recovery and radio DFU are unavailable. See [recovery prerequisites](../README.md#recovering-an-interrupted-firmware-update), [settings and batch plans](../docs/settings-protocol.md), and [Bluetooth limitations](../docs/bluetooth.md).

## Build locally

Requires Rust 1.90+, its WASM target, and Node.js 22 with npm. Run from the repository root:

```sh
rustup target add wasm32-unknown-unknown
RUSTFLAGS="--remap-path-prefix=$PWD=. --remap-path-prefix=$HOME=/build-home" \
  cargo build --release --locked --target wasm32-unknown-unknown --lib
cargo install wasm-bindgen-cli --locked --version 0.2.129
wasm-bindgen target/wasm32-unknown-unknown/release/cranklink.wasm \
  --out-dir web-instrument/pkg --target web
cd web-instrument
npm ci
npm run build
npm run preview
```

Open the displayed local URL on port 8030. The pinned wasm-bindgen CLI matches the Rust lockfile. Rebuild the WASM package after Rust changes, then rebuild the web instrument. Deploy the complete `dist` output, including its WASM runtime and service worker assets.

## Verify

From `web-instrument`, after the build:

```sh
npm test
npx playwright install chromium
npm run test:production
```

The production suite serves the exact `dist` output at `/cranklink/` without isolation headers and checks service-worker isolation, WASM startup, and the simulated settings workflow. This validates the published artifact and subpath; it does not test real adapters, motors, or Bluetooth radios.
