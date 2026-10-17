# Agent Instructions

- **Documentation Sync**: Whenever modifying CLI commands, options, or core features, always update `README.md` (and relevant files in `docs/`) to keep usage examples and command trees in sync with the implementation.
- **Fast Web Instrument Test Cycle**: When developing or testing in `web-instrument`, use the rapid iteration tiers by default:
  - `npm run test:fast` (<100ms): Fast domain schemas, firmware package rules, and SessionController FSM tests.
  - `npm run test:smoke` (~2s): Fast smoke E2E test verifying connect, inline staging, atomic batch commit, and rollback.
  - `npm run test:e2e` (~7s): Full parallel wire-level GATT simulator, auth, WebMCP, and mobile responsive test suite.
  - `npm run test:production` (~1s): Reserved for pre-push or CI to verify production build assets, subpath hosting, and service worker COOP/COEP isolation.
