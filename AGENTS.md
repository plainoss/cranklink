# Agent Instructions

## 1. Documentation Sync
- **CLI & Core Docs**: Whenever modifying CLI commands, options, or core features, always update `README.md` (and relevant files in `docs/`) to keep usage examples and command trees in sync with the implementation.

## 2. Tooling & Local Verification
Always prefer fast local tooling over slow manual loops:
- **Unified Fast Check**: Run `./tools/verify.sh` to check formatting, the native and wasm32 Rust builds, Rust unit tests, the firmware simulator/patcher tests (when `unicorn` is importable; set `CRANKLINK_SIM_PYTHON` to a venv), web TypeScript types, and web unit tests in one command.
- **Git Pre-commit Hook**: Run `./tools/install-git-hooks.sh` once in any worktree to enable `.githooks/pre-commit` (guarantees formatting and types before commit).
- **Fast Web Typecheck**: In `web-instrument`, run `npm run typecheck` (`tsc --noEmit`, <1s) instead of the full `npm run build` bundle.
- **Fast Web Instrument Test Cycle**: When developing or testing in `web-instrument`, use the rapid iteration tiers:
  - `npm run test:fast` (<100ms): Fast domain schemas, firmware package rules, and SessionController FSM tests.
  - `npm run test:smoke` (~2s): Fast smoke E2E test verifying connect, inline staging, ordered batch commit, and rollback.
  - `npm run test:e2e` (~7s): Full parallel wire-level GATT simulator, auth, WebMCP, and mobile responsive test suite.
  - `npm run test:production` (~1s): Reserved for pre-push or CI to verify production build assets and `_headers` COOP/COEP isolation.
- **E2E Port Safety**: `npm run test:smoke` and `npm run test:e2e` use port 8030 and first run `scripts/ensure-port-free.mjs`, which stops only a leftover server started from this worktree's `web-instrument/` and refuses when another directory holds the port. Set `CRANKLINK_E2E_PORT=<port>` to run worktrees side by side; `CRANKLINK_E2E_REUSE=1` reuses this tree's running server.

## 3. Worktree Setup Protocol
- When starting work in a new git worktree, if `web-instrument/node_modules` does not exist, run `npm ci` in `web-instrument/` before running web commands.

## 4. CI & Subagent Execution Rules
- **No CI Busy-Polling**: Never run `gh pr checks` in loops (it exits with code 1 while checks are still pending). Use `./tools/ci-watch.sh [PR_NUM]` or `gh pr checks --watch`, or launch as a background task.
- **No Subagent Polling**: Never run `manage_subagents Action="list"` in polling loops; the agent environment automatically delivers messages and wakeups when subagents complete or reply.
- **Scratch Scripts**: When doing firmware analysis, disassembly, or complex data processing, write temporary scripts in `<conversation-id>/scratch/` or `tools/` instead of executing fragile inline multi-line Python snippets in shell commands.

## 5. Concurrent PRs & Merge Queue Protocol
- **Auto-Merge to Queue**: After creating a PR, mark it for auto-merging:
  ```bash
  gh pr merge <PR_NUMBER> --auto --squash
  # Or use the helper:
  ./tools/queue-pr.sh [PR_NUMBER]
  ```
- **Speculative Merge Testing**: The repository enforces a Merge Queue on `main`. Once required checks pass, GitHub speculatively tests the PR in a batch and merges it to `main` without manual intervention.
- **No Manual Rebase Stampedes**: If another PR merges while yours is in flight, do not frantically rebase in loops. The merge queue automatically handles ordering and speculative merges.
