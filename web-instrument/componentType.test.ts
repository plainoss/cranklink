import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { componentLabel, componentType } from "./src/domain/componentType.ts";

describe("component types", () => {
  it("names the type from the model prefix", () => {
    assert.equal(componentType("DU-EP800-CRG"), "Motor");
    assert.equal(componentType("RD-M8050-GS"), "Rear derailleur");
    assert.equal(componentType("SW-E7000-R"), "Shifter");
    assert.equal(componentType("ST-6870-L"), "Shifter");
    assert.equal(componentType("BT-DN110"), "Battery");
    assert.equal(componentType("SC-EM800"), "Display");
    assert.equal(componentType("EW-WU111"), "Wireless unit");
    assert.equal(componentType("EW-EN100"), "Junction");
    assert.equal(componentType("SM-BMR1"), "Battery mount");
    assert.equal(componentType("XX-1"), null);
    assert.equal(componentType(null), null);
  });

  it("labels models with their type", () => {
    assert.equal(componentLabel(["DU-EP800-CRG"]), "Motor · DU-EP800-CRG");
    assert.equal(componentLabel(["SW-E7000-R", "SW-EM800-L"]), "Shifter · SW-E7000-R / SW-EM800-L");
    assert.equal(componentLabel(["SW-E7000-R", "SC-EM800"]), "SW-E7000-R / SC-EM800");
    assert.equal(componentLabel(["XX-1"]), "XX-1");
  });
});
