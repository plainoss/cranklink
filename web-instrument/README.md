# Browser instrument

Open [cranklink](https://cranklink.plainoss.workers.dev/). The browser instrument uses the same Rust core as the native CLI, compiled to WASM. It provides component discovery and motor identity, firmware inspection, schema-supported settings reads/writes, serial-bound backups and restore, plus a command terminal for ordered read/set/restore batches. The responsive layout supports phone, tablet, and desktop screens, with navigation and staged-change review adapted to smaller displays.

Use a browser with the required WebUSB, Web Serial, or Web Bluetooth API, such as Chrome or Edge where supported. Browser and operating-system support determine which transports work; a phone supporting one API may lack another. The connect screen lists which connections the current browser supports. Development builds (`npm run dev`) add a simulator that emulates a Bluetooth gateway and drive unit, with optional fault injection, for exploring settings without hardware; published builds omit it. A compatible BLE gateway is required for wireless access. Allow site storage.

On Android, connect the SM-PCE02 with **USB** (WebUSB). The adapter's `0x0A00` device revision is supported at 2 Mbaud. If its internal supercapacitor needs charging, the connection screen shows progress and waits up to 90 seconds before continuing. Keep the USB cable connected; a timeout reports the power state and asks you to check the USB power source.

Firmware is not included. Wired flashing/recovery retains the CLI's model, board, serial, and companion checks. BLE settings and opt-in firmware relay are experimental; BLE recovery and radio DFU are unavailable. See [recovery prerequisites](../README.md#recovering-an-interrupted-firmware-update), [settings and batch plans](../docs/settings-protocol.md), and [Bluetooth limitations](../docs/bluetooth.md).

> [!WARNING]
> **Unofficial Software & Firmware Flashing Warning**: `cranklink` is independent, unofficial software not endorsed by Shimano. Working directly with drive unit hardware and firmware is inherently risky and may brick components or void warranties. Firmware flashing has **only been tested on a DU-EP800-CRG**, does **not verify firmware compatibility** against your hardware revision, is experimental, and is targeted strictly at users who know what they are doing.

The speed slider and presets use the full model-supported range (15–32 km/h on EP800, 14–45 km/h on EP600/EP801). Choosing a speed stages a compatible destination when the current one cannot support it; both changes appear in the save review. On EP500/EP600/EP801, profile torque is chosen from the motor's accepted basic-mode levels. Tire circumference accepts a typed, measured roll-out in millimetres; the buttons nudge it by the unit's step. Returning the speed to its previous value removes a destination change added only for that speed. Explicitly selecting a destination adjusts the staged speed into that destination's range. A rejected save keeps the settings screen and staged values available for correction.

Every setting row shows its live value, any staged value, and an undo control; settings a drive unit does not report are hidden. All Settings lists every setting the drive unit reports, including ones without a dedicated control, and search covers both. Saving always opens a review that names each change with units, allows undoing single changes, and requires confirmation for region or OEM factory-bound changes. After a save, a confirmation links to the backup of the previous values. If a save stops, the banner states how many changes were applied and which one may be uncertain before giving the cause; when the link has closed, Reconnect reopens the session and keeps staged values for the same drive unit. Backup & Restore groups backups by the save, restore, or full backup that created them and restores a group in one step after confirmation. Diagnostics shows each command once with collapsible long output.

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

Open the displayed local URL on port 8030. The pinned wasm-bindgen CLI matches the Rust lockfile. Rebuild the WASM package after Rust changes, then rebuild the web instrument. `npm run build:cloudflare` performs the whole sequence, bootstrapping the pinned Rust toolchain and wasm-bindgen when missing.

## Verify

From `web-instrument`, after the build:

```sh
# Fast dev tier: domain schemas, firmware package checks, and SessionController FSM (<100ms)
npm run test:fast

# Fast smoke E2E: connect, inline staging, atomic batch commit, and snapshot rollback (~2s)
npm run test:smoke

# Full parallel E2E: all wire-level GATT simulator, authentication, WebMCP, and mobile views (~7s)
npm run test:e2e

# Production suite: serves dist with only its _headers rules, verifying COOP/COEP isolation
npx playwright install chromium
npm run test:production
```

The production suite serves the exact `dist` output with only the headers declared in its `_headers` file, as Cloudflare Workers does, and checks cross-origin isolation, WASM startup, and the production UI controls. Set `PLAYWRIGHT_BASE_URL` to run it against a deployed preview. It does not test real adapters, motors, or Bluetooth radios.

## Deploy

[Cloudflare Workers Builds](https://developers.cloudflare.com/workers/ci-cd/builds/) builds and deploys the instrument as a static-assets Worker (`wrangler.jsonc`). Pushes to `main` deploy to production at <https://cranklink.plainoss.workers.dev/>; every other branch gets its own preview on `*-cranklink.plainoss.workers.dev`, linked from the Workers Builds check on its commit. `_headers` supplies the COOP/COEP isolation headers. The Workers Builds project uses:

| Setting | Value |
|---|---|
| Root directory | `web-instrument` |
| Build command | `npm run build:cloudflare` |
| Deploy / preview command | `npx wrangler deploy` / `npx wrangler preview` |
| Build variable | `SKIP_DEPENDENCY_INSTALL=1` |

To deploy a preview manually after a local build: `npx wrangler preview --name <name>`.
