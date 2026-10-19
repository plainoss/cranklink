import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "dist");
const copy = (name) => {
  const source = path.join(root, name);
  if (!fs.statSync(source).isFile()) throw new Error(`Missing runtime file: ${name}`);
  const target = path.join(dist, name);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.copyFileSync(source, target);
};

// A package-relative host import proves the glue was rebuilt for subpath hosting.
const glue = fs.readFileSync(path.join(root, "pkg/cranklink.js"), "utf8");
if (!glue.includes("../host.js") || /["']\/host\.js["']/.test(glue)) {
  throw new Error("Rebuild the WASM package before publishing: host imports must be relative.");
}
const canonical = fs.readFileSync(path.join(root, "node_modules/coi-serviceworker/coi-serviceworker.min.js"));
if (!canonical.equals(fs.readFileSync(path.join(root, "coi-serviceworker.js")))) {
  throw new Error("Service worker must match the pinned coi-serviceworker dependency.");
}
// _headers sets COOP/COEP natively on Cloudflare Workers.
for (const name of [
  "_headers", "worker.js", "host.js", "coi-serviceworker.js",
  "licenses/coi-serviceworker-LICENSE.txt",
  "pkg/cranklink.js", "pkg/cranklink_bg.wasm",
  "schemas/ep800-crg.json", "schemas/ep600.json", "schemas/ep801.json", "schemas/ep500.json",
  "schemas/e5100.json", "schemas/e8000.json", "schemas/e7000.json", "schemas/e6100.json", "schemas/e6000.json",
]) copy(name);
fs.writeFileSync(path.join(dist, ".nojekyll"), "");
console.log("Copied required runtime assets and service-worker license.");
