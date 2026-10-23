#!/usr/bin/env node
// Regenerate the static motor schemas the web app loads before falling back to
// WASM. `cargo test` fails when these files drift from the Rust tables.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");
const schemasDir = resolve(root, "web-instrument", "schemas");

const models = ["ep800-crg", "ep600", "ep801", "ep500", "e5100", "e8000", "e7000", "e6100", "e6000", "e6001", "e6002-pre33", "e6002"];

for (const model of models) {
  const output = execFileSync(
    "cargo",
    [
      "run", "--quiet", "--locked", "--manifest-path", resolve(root, "Cargo.toml"), "--bin", "cranklink", "--",
      "--json", "settings", "schema", "--model", model,
    ],
    { encoding: "utf-8" },
  );
  JSON.parse(output);
  const target = resolve(schemasDir, `${model}.json`);
  writeFileSync(target, output.trim() + "\n", "utf-8");
  console.log(`Synced schema for ${model} -> ${target}`);
}
