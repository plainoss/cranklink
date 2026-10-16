#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, "..");
const schemasDir = resolve(root, "web", "schemas");
mkdirSync(schemasDir, { recursive: true });

const models = ["ep800-crg", "ep600", "ep801"];

for (const model of models) {
  const output = execFileSync(
    "cargo",
    ["run", "--quiet", "--manifest-path", resolve(root, "Cargo.toml"), "--", "settings", "schema", "--model", model],
    { encoding: "utf-8" }
  );
  // Verify valid JSON
  JSON.parse(output);
  const target = resolve(schemasDir, `${model}.json`);
  writeFileSync(target, output.trim() + "\n", "utf-8");
  console.log(`Synced schema for ${model} -> ${target}`);
}
