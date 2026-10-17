import { it } from "node:test";
import assert from "node:assert/strict";
import { WebBleGatt } from "./ble.js";

it("rejected gateway authentication clears the cached passkey and prompts again", async () => {
  const device = Object.assign(new EventTarget(), {
    id: "test-gateway",
    name: "Test gateway",
    gatt: { connected: false },
  });
  let promptCount = 0;
  let resetCount = 0;
  const gatt = new WebBleGatt(
    device,
    async () => String(++promptCount).repeat(6),
    () => {
      resetCount++;
      assert.equal(gatt.credential, null, "credential is cleared before notifying the host");
    },
  );
  const empty = new Uint8Array();
  const requestPasskey = () => gatt.request("ble.passkey", empty, 1000);
  // Authentication requests require an open connection; no radio is used here.
  gatt.opened = true;
  const firstCredential = await requestPasskey();
  assert.deepEqual(await requestPasskey(), firstCredential);
  assert.equal(promptCount, 1, "successful requests reuse the cached credential");

  assert.deepEqual(await gatt.request("ble.reset-passkey", empty, 1000), empty);
  assert.equal(gatt.credential, null);
  assert.equal(resetCount, 1, "the host cache is invalidated once per reset");

  const retryCredential = await requestPasskey();
  assert.equal(promptCount, 2, "a rejected credential must prompt again");
  assert.notDeepEqual(retryCredential, firstCredential);
  assert.deepEqual(await requestPasskey(), retryCredential);
  assert.equal(promptCount, 2, "the replacement credential can be reused");
  assert.equal(resetCount, 1);
  await gatt.forget();
});
