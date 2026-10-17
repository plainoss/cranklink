# Browser instrument

Open [cranklink on GitHub Pages](https://plainoss.github.io/cranklink/). The browser instrument uses the same Rust core as the native CLI, compiled to WASM. It provides component discovery and motor identity, firmware inspection, schema-supported settings reads/writes, serial-bound backups and restore, plus a command terminal for ordered read/set/restore batches. The responsive layout supports phone, tablet, and desktop screens, with navigation and staged-change review adapted to smaller displays.

Use a browser with the required WebUSB, Web Serial, or Web Bluetooth API, such as Chrome or Edge where supported. Browser and operating-system support determine which transports work; a phone supporting one API may lack another. Unavailable device buttons are disabled with an explanation; the simulator remains available to explore settings without hardware. A compatible BLE gateway is required for wireless access. The first visit may reload once while a service worker enables the isolation needed by WASM. Allow site storage and service workers.

On Android, connect the SM-PCE02 with **USB** (WebUSB). The adapter's `0x0A00` device revision is supported at 2 Mbaud. If its internal supercapacitor needs charging, the connection screen shows progress and waits up to 90 seconds before continuing. Keep the USB cable connected; a timeout reports the power state and asks you to check the USB power source.

Firmware is not included. Wired flashing/recovery retains the CLI's model, board, serial, and companion checks. BLE settings and opt-in firmware relay are experimental; BLE recovery and radio DFU are unavailable. See [recovery prerequisites](../README.md#recovering-an-interrupted-firmware-update), [settings and batch plans](../docs/settings-protocol.md), and [Bluetooth limitations](../docs/bluetooth.md).

> [!WARNING]
> **Unofficial Software & Firmware Flashing Warning**: `cranklink` is independent, unofficial software not endorsed by Shimano. Working directly with drive unit hardware and firmware is inherently risky and may brick components or void warranties. Firmware flashing has **only been tested on a DU-EP800-CRG**, does **not verify firmware compatibility** against your hardware revision, is experimental, and is targeted strictly at users who know what they are doing.

The speed slider and presets use the full model-supported range (15–32 km/h on EP800, 14–45 km/h on EP600/EP801). Choosing a speed stages a compatible destination when the current one cannot support it; both changes appear in the save review. Returning the speed to its previous value removes a destination change added only for that speed. Explicitly selecting a destination adjusts the staged speed into that destination's range. A rejected save keeps the settings screen and staged values available for correction.

For Bluetooth discovery, turn the bike on just before connecting. If it is not listed, turn it off and on again, then enable the display or gateway's Bluetooth connection mode if needed. Close other apps connected to the bike.

The instrument selects settings from the detected motor family: EP800, EP600, EP801, EP500, E5100, E8000/E8080, E7000, E6100-family, or E6000-family. Older E-series schemas expose wheel circumference, mounting angle, and lighting only; unsupported speed and profile controls are hidden. E5100 includes common settings and speed controls. Shared-series variants use the verified product name when available; ambiguous E6000 products retain a family label.

Initial BLE connection selects master routing before reading motor identity and settings together and keeps the authenticated connection between commands. That route remains active for subsequent saves and automatic reads until Disconnect. Each command releases motor PC-link ownership afterward; the next command confirms the retained route and inventory, freshly checks motor identity, and reestablishes motor ownership and any required authorization. Explicit Disconnect restores normal routing, stops the gateway session, and closes GATT; an operation failure closes the connection without replaying the command. Enable the bike's connection mode before reconnecting. Rejected gateway PINs are cleared so the next connection prompts again. Known Bluetooth errors show a short recovery message; full details remain in Diagnostics.

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
# Fast dev tier: domain schemas, firmware package checks, and SessionController FSM (<100ms)
npm run test:fast

# Fast smoke E2E: connect, inline staging, atomic batch commit, and snapshot rollback (~2s)
npm run test:smoke

# Full parallel E2E: all wire-level GATT simulator, authentication, WebMCP, and mobile views (~7s)
npm run test:e2e

# Production suite: serves dist at /cranklink/ verifying service-worker COOP/COEP isolation
npx playwright install chromium
npm run test:production
```

The production suite serves the exact `dist` output at `/cranklink/` without isolation headers and checks service-worker isolation, WASM startup, and the production UI controls. This validates the published artifact and subpath; it does not test real adapters, motors, or Bluetooth radios.
