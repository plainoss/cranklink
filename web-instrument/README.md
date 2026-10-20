# Web app development

User-facing usage is in the [main README](../README.md#web-app).

## Build

Requires Node.js 22. From `web-instrument/`:

```sh
npm ci
npm run build:cloudflare   # pinned Rust toolchain -> WASM core -> dist/
npm run dev                # dev server, includes a hardware simulator
```

On macOS, install wasm-bindgen first: `cargo install wasm-bindgen-cli --version 0.2.129 --locked`. After Rust changes, rerun `build:cloudflare`. The dev simulator (Bluetooth gateway and drive unit, with fault injection) is omitted from production builds.

## Test

```sh
npm run test:fast         # unit tests (<100ms)
npm run test:smoke        # connect, stage, save, rollback (~2s)
npm run test:e2e          # full browser suite against the GATT simulator (~7s)
npm run test:production   # built dist with only its _headers (COOP/COEP); needs `npx playwright install chromium`
```

`PLAYWRIGHT_BASE_URL` points `test:production` at a deployed preview. None of these exercise real adapters or radios.

## Deploy

[Cloudflare Workers Builds](https://developers.cloudflare.com/workers/ci-cd/builds/) builds and deploys the web app as a static-assets Worker (`wrangler.jsonc`). Pushes to `main` deploy to production at <https://cranklink.plainoss.workers.dev/>; every other branch gets its own preview on `*-cranklink.plainoss.workers.dev`, linked from the Workers Builds check on its commit. `_headers` supplies the COOP/COEP isolation headers. The Workers Builds project uses:

| Setting | Value |
|---|---|
| Root directory | `web-instrument` |
| Build command | `npm run build:cloudflare` |
| Deploy / preview command | `npx wrangler deploy` / `npx wrangler preview` |
| Build variable | `SKIP_DEPENDENCY_INSTALL=1` |

To deploy a preview manually after a local build: `npx wrangler preview --name <name>`.
