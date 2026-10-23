# Agent Instructions

## 1. Documentation Sync
- **CLI & Core Docs**: Whenever modifying CLI commands, options, or core features, always update `README.md` (and relevant files in `docs/`) to keep usage examples and command trees in sync with the implementation.

## 2. Tooling & Local Verification
Always prefer fast local tooling over slow manual loops:
- **Unified Fast Check**: Run `./tools/verify.sh` for `cargo fmt --check`, `cargo check` of the USB-only and wasm32 builds, the Rust unit tests (default BLE build), the firmware simulator/patcher tests, the web typecheck and the web unit tests in one command. It prints "Skipped" and carries on when rustfmt, the wasm32 target or `unicorn` is missing (set `CRANKLINK_SIM_PYTHON` to a venv python). CI also runs the USB-only tests, the production web build with its asset check (`npm run build:cloudflare` in `web-instrument` reproduces both) and the Playwright suites, which verify.sh does not.
- **Git Pre-commit Hook**: `npm ci` in `web-instrument` (its `prepare` script) or `./tools/install-git-hooks.sh` sets `core.hooksPath` to `.githooks`, enabling `.githooks/pre-commit`. It runs `cargo fmt --check` when Rust files are staged and `npm run typecheck` when TypeScript under `web-instrument/` is staged; without `web-instrument/node_modules` it runs only `cargo fmt --check`, whatever is staged. It never compiles or tests Rust, so run `./tools/verify.sh` as well. Its nano-staged run hides unstaged hunks of staged files, but both checks read the whole working tree, so a commit that depends on unstaged or untracked edits to other files can still pass.
- **Fast Web Typecheck**: In `web-instrument`, run `npm run typecheck` (`tsc --noEmit`, <1s) instead of the full `npm run build` bundle.
- **Fast Web Instrument Test Cycle**: When developing or testing in `web-instrument`, use the rapid iteration tiers:
  - `npm run test:fast` (<100ms): Fast domain schemas, firmware package rules, and SessionController FSM tests.
  - `npm run test:smoke` (~2s): Fast smoke E2E test verifying connect, inline staging, ordered batch commit, and rollback.
  - `npm run test:e2e` (~7s): Full parallel wire-level GATT simulator, auth, WebMCP, and mobile responsive test suite.
  - `npm run test:production` (~1s): Serves the built `dist/` with only the headers its `_headers` file declares and checks COOP/COEP isolation, WASM startup and that the dev simulator is omitted. CI runs it after the build; locally run `npm run build:cloudflare` first (or `npm run build` when `pkg/` is already fresh).
- **E2E Port Safety**: `npm run test:smoke` and `npm run test:e2e` use port 8030 and first run `scripts/ensure-port-free.mjs`, which stops only a leftover server started from this worktree's `web-instrument/` and refuses when another directory holds the port. Set `CRANKLINK_E2E_PORT=<port>` to run worktrees side by side; `CRANKLINK_E2E_REUSE=1` reuses this tree's running server.
- **Schema Sync**: After changing a motor settings table, run `node --experimental-strip-types tools/sync-schemas.ts` from the repository root; `cargo test` fails while `web-instrument/schemas/` differs from the Rust tables.

## 3. Worktree Setup Protocol
- When starting work in a new git worktree, if `web-instrument/node_modules` does not exist, run `npm ci` in `web-instrument/` before running web commands.

## 4. CI & Subagent Execution Rules
- **No CI Busy-Polling**: Never run `gh pr checks` in loops (it exits with code 8 while checks are still pending). Use `./tools/ci-watch.sh [PR_NUM]` or `gh pr checks --watch`, or launch as a background task.
- **No Subagent Polling**: Never run `manage_subagents Action="list"` in polling loops; the agent environment automatically delivers messages and wakeups when subagents complete or reply.
- **Scratch Scripts**: When doing firmware analysis, disassembly, or complex data processing, write temporary scripts in `scratch/` at the repository root (gitignored) instead of executing fragile inline multi-line Python snippets in shell commands.

## 5. Concurrent PRs & Merge Queue Protocol
- **Never Queue Your Own PR**: After creating a PR, do not enable auto-merge or add it to the merge queue (`gh pr merge --auto`, `./tools/queue-pr.sh`). Ask for review; a maintainer, or a review agent they set up, approves and queues it. `tools/queue-pr.sh` is for maintainers.
- **Speculative Merge Testing**: The repository enforces a Merge Queue on `main`. Once a PR is queued and required checks pass, GitHub speculatively tests it in a batch and merges it to `main` without manual intervention.
- **No Manual Rebase Stampedes**: If another PR merges while yours is in flight, do not frantically rebase in loops. The merge queue automatically handles ordering and speculative merges.
