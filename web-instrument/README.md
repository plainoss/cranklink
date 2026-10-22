# Web app development

User-facing usage is in the [main README](../README.md#web-app).

## Build

Requires Node.js 22. From `web-instrument/`:

```sh
npm ci
npm run build:cloudflare   # pinned Rust toolchain -> WASM core -> dist/
npm run dev                # dev server, includes a hardware simulator
```

On macOS, install wasm-bindgen first: `cargo install wasm-bindgen-cli --version 0.2.129 --locked`. After Rust changes, rerun `build:cloudflare`. After changing a motor settings table, run `node --experimental-strip-types tools/sync-schemas.ts` from the repository root to regenerate `schemas/`; `cargo test` fails while they differ. The dev simulator (Bluetooth gateway and drive unit, with optional Di2 components and fault injection) is omitted from production builds.

The UI inventories the bus and loads the selected component's schema from the Rust core. Motor schemas retain their curated tuning views; nonmotor components use schema-based controls with read-only guards. Saves and restores carry the selected slot, model and serial. Inventory refresh and target changes are blocked while edits are staged or operations are running. Bluetooth connection help covers both STEPS power/button windows and Di2 function/Junction A buttons.

## Test

```sh
npm run test:fast         # unit tests (<100ms)
npm run test:smoke        # connect, stage, save, rollback (~2s)
npm run test:e2e          # full browser suite against the GATT simulator (~20s)
npm run test:production   # built dist with only its _headers (COOP/COEP); needs `npx playwright install chromium`
```

`PLAYWRIGHT_BASE_URL` points `test:production` at a deployed preview. None of these exercise real adapters or radios.

`components.spec.ts` exercises a mixed bus with levers, derailleurs, a power unit, hub actuator and display through the actual WASM client. For unchanged stock-firmware browser execution, supply local exact-hash SC-EM800 4.6.0 and MU-UR500 3.3.1 images in a JSON manifest mapping profile IDs to absolute paths:

```sh
CRANKLINK_COMPONENT_BROWSER_FIXTURES=/tmp/component-fixtures.json \
CRANKLINK_SIM_PYTHON=/tmp/cranklink-firmware-sim/bin/python \
CRANKLINK_COMPONENT_BROWSER_REPORT_DIR=/tmp/component-browser-results \
  npx playwright test --config=playwright.stock-components.config.ts
```

Build `dist/` first; this configuration serves the production assets with their declared isolation headers. These opt-in tests run the current UI, WebBleGatt and WASM against `tools/firmware-sim/component_browser_peer.py`. They verify durable backups before real SETs, stock commits, modeled power-cycle persistence, restore and safe uncertainty for unfinished hub actuation. They skip without local firmware; scripted tests do not count as stock-firmware execution. See [execution boundaries](../docs/firmware-simulator.md#component-settings-execution).

## Agent access (WebMCP)

The page answers a small Model Context Protocol surface for in-page agents: JSON-RPC 2.0 messages posted with `window.postMessage` from the same window or the same origin (other origins are ignored), or direct calls to `window.mcp.handleRequest(method, params)`. Methods are `tools/list`, `tools/call`, `resources/list` and `resources/read`; resources are `cranklink://device/identity` and `cranklink://device/settings`.

| Tool | Effect |
| --- | --- |
| `cranklink_list_components`, `cranklink_select_component` | Inventory and target selection; refused while edits are staged or an operation runs |
| `cranklink_probe`, `cranklink_get_schema` | Active component identity and schema |
| `cranklink_read_settings`, `cranklink_create_backup` | Read values or save a snapshot; refused unless the component is connected and idle |
| `cranklink_set_setting`, `cranklink_batch_settings` | Request writes; nothing is sent until the operator authorizes them in a banner that names the target model, slot and serial |

`esteps_*` names are aliases. Agent writes are refused while operator edits are staged, while another review is pending, or when the link is lost; a pending review is rejected on disconnect. Approved writes go through the same backed-up, verified batch save as the UI.

## Deploy

[Cloudflare Workers Builds](https://developers.cloudflare.com/workers/ci-cd/builds/) builds and deploys the web app as a static-assets Worker (`wrangler.jsonc`). Pushes to `main` deploy to production at <https://cranklink.plainoss.workers.dev/>; every other branch gets its own preview on `*-cranklink.plainoss.workers.dev`, linked from the Workers Builds check on its commit. `_headers` supplies the COOP/COEP isolation headers. The Workers Builds project uses:

| Setting | Value |
|---|---|
| Root directory | `web-instrument` |
| Build command | `npm run build:cloudflare` |
| Deploy / preview command | `npx wrangler deploy` / `npx wrangler preview` |
| Build variable | `SKIP_DEPENDENCY_INSTALL=1` |

To deploy a preview manually after a local build: `npx wrangler preview --name <name>`.
