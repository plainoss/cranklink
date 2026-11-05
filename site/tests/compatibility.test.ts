import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { readCompatibility } from "../src/lib/compatibility.ts";

const readme = readFileSync(
  new URL("../../README.md", import.meta.url),
  "utf8",
);

test("both README tables are loaded, including later models and unsupported families", () => {
  const { motors, parts } = readCompatibility(readme);
  assert.ok(
    motors.rows.some(
      (row) =>
        row[0].includes("DU-E5000") && row[2].includes("identified only"),
    ),
  );
  assert.ok(parts.rows.some((row) => row[0].includes("Di2 batteries")));
  assert.ok(motors.rows.some((row) => row[4].includes("¹")));
  assert.ok(motors.rows.some((row) => row[4].includes("²")));
});

test("README column changes fail rather than silently rendering misleading labels", () => {
  assert.throws(
    () =>
      readCompatibility(readme.replace("| Motor firmware |", "| Unknown |")),
    /columns changed/,
  );
});

test("missing tables, malformed rows and separator changes fail the build", () => {
  assert.throws(
    () => readCompatibility(readme.replace("### Motors\n", "### Bikes\n")),
    /heading missing/,
  );
  assert.throws(
    () =>
      readCompatibility(
        readme.replace("| Yes, tested |", "| Yes | Unexpected |"),
      ),
    /rows.*malformed/,
  );
  assert.throws(
    () =>
      readCompatibility(
        readme.replace(
          "| :--- | :--- | :--- | :--- | :--- |",
          "| invalid separator |",
        ),
      ),
    /separator missing/,
  );
});
