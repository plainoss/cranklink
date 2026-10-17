# Agent Instructions

## 1. Documentation Sync
- **CLI & Core Docs**: Whenever modifying CLI commands, options, or core features, always update `README.md` (and relevant files in `docs/`) to keep usage examples and command trees in sync with the implementation.

## 2. Codebase Architecture & File Map
Use this map to locate symbols directly without reading entire monolithic files:
- **`src/cli.rs`**: Clap CLI definitions, subcommand flags, and execution handlers (`probe`, `units`, `settings`, `firmware`, `adapter`, `dump`, `ble`).
- **`src/device.rs`**: FTDI SM-PCE02 USB adapter transport, power rail management, and packet transactions.
- **`src/ble.rs`**: Bluetooth Low Energy transport, adapter routing, peripheral connections.
- **`src/rx.rs`**: Frame reception, window buffers, checksum validation, and transfer envelopes.
- **`src/settings.rs`**: Motor settings codec, speed/torque tuning schemas, and batch backups.
- **`src/firmware.rs` & `src/firmware_io.rs`**: Dual-MCU firmware flashing and verification protocol.
- **`src/unit_models.rs`**: Shimano drive-unit model catalog and identity resolution.
- **`web-instrument/src/App.tsx`**: Main UI application shell, navigation, and top-level modals.
- **`web-instrument/src/core/sessionController.ts`**: Central state machine and bridge to WASM/worker.
- **`web-instrument/src/components/views/`**: View panels (`ConnectView`, `TuneView`, `OverviewView`, `FirmwareView`, `BackupView`, `DiagnosticsView`).

## 3. Tooling & Local Verification
Guaranteed tooling is provided in `tools/` and `web-instrument/package.json`:
- **Unified Fast Check**: Run `./tools/verify.sh` to check formatting, Rust types, Rust unit tests, web TypeScript types, and web unit tests in one command.
- **Git Pre-commit Hook**: Run `./tools/install-git-hooks.sh` once in any worktree to enable `.githooks/pre-commit` (checks formatting and types on commit).
- **Fast Web Typecheck**: In `web-instrument`, run `npm run typecheck` (`tsc --noEmit`, <1s) before or instead of running full `npm run build`.
- **E2E Port Safety**: `npm run test:e2e` automatically invokes `scripts/ensure-port-free.mjs 8030` to prevent port collisions across worktrees.

## 4. Worktree Setup Protocol
- When starting work in a new git worktree, if `web-instrument/node_modules` does not exist, run `npm ci` in `web-instrument/` before running any web commands.

## 5. CI & Subagent Execution Rules
- **No CI Busy-Polling**: Never run `gh pr checks` in loops (it exits with code 1 while checks are still pending). Use `./tools/ci-watch.sh [PR_NUM]` or `gh pr checks --watch`, or launch as a background task.
- **No Subagent Polling**: Never run `manage_subagents Action="list"` in polling loops; the agent environment automatically delivers messages and wakeups when subagents complete or reply.
- **Scratch Scripts**: When doing firmware analysis, disassembly, or complex data processing, write temporary scripts in `<conversation-id>/scratch/` or `tools/` instead of executing fragile inline multi-line Python snippets in shell commands.
