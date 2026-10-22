import { it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { OPEN_WORST_CASE_MS, WebBleGatt, characteristicUuid } from "./ble.js";

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

// Models a gateway that ignores disconnect() while a connect is pending, so a
// timed-out connect can still complete later. "settle" resolves every pending
// connect together with this one.
function fakeGateway(plan: Array<"reject" | "hang" | "settle">) {
  const pending: Array<() => void> = [];
  const characteristics = [
    0x2af3, 0x2af4, 0x2af9, 0x2afa, 0x2afd, 0x2afe, 0x2aff,
  ].map((id) =>
    Object.assign(new EventTarget(), {
      uuid: characteristicUuid(id),
      properties: { read: true, write: true, notify: true },
      startNotifications: async () => {},
    }),
  );
  const server = {
    getPrimaryService: async () => ({
      getCharacteristics: async () => characteristics,
    }),
  };
  const link = {
    connected: false,
    connects: 0,
    disconnects: 0,
    connect() {
      const step = plan[link.connects++];
      if (step === "reject")
        return Promise.reject(new Error("Connection failed for unknown reason."));
      if (step === "settle") setTimeout(linkUp, 0);
      return new Promise((resolve) => pending.push(() => resolve(server)));
    },
    disconnect() {
      link.disconnects++;
      link.connected = false;
    },
  };
  const linkUp = () => {
    link.connected = true;
    for (const resolve of pending.splice(0)) resolve();
  };
  const device = Object.assign(new EventTarget(), {
    id: "test-gateway",
    name: "Test gateway",
    gatt: link,
  });
  const ble = new WebBleGatt(device, async () => "000000");
  ble.connectTimeoutMs = 20;
  ble.connectBackoffMs = 1;
  return { ble, link, linkUp };
}

it("a connect retry after a failed attempt yields a usable session", async () => {
  const { ble, link } = fakeGateway(["reject", "settle"]);
  await ble.open();
  assert.equal(link.connects, 2);
  assert.equal(ble.failure, null, "the failed attempt must not poison the retry");
  await ble.subscribe(0x2af9);
  await ble.forget();
});

it("a timed-out connect completing late keeps the retry's link", async () => {
  const { ble, link } = fakeGateway(["hang", "settle"]);
  await ble.open();
  assert.equal(link.connects, 2);
  assert.equal(link.connected, true, "the late attempt must not close the link");
  await ble.subscribe(0x2af9);
  await ble.forget();
});

it("a connect completing after every attempt timed out is closed", async () => {
  const { ble, link, linkUp } = fakeGateway(["hang", "hang", "hang"]);
  await assert.rejects(ble.open(), /timed out/);
  linkUp();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(link.connected, false, "an abandoned connect must not stay up");
  assert.equal(ble.opened, false);
});

it("releasing the device during a pending connect aborts the open", async () => {
  const { ble, link, linkUp } = fakeGateway(["hang"]);
  ble.connectTimeoutMs = 1000;
  const opening = ble.open();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await ble.forget();
  linkUp();
  await assert.rejects(opening, /closed while opening/);
  assert.equal(link.connects, 1);
  assert.equal(ble.opened, false);
  assert.equal(link.connected, false, "a released object must not keep the link");
});

it("the Rust ble.open budget covers open()'s worst case", () => {
  const source = fs.readFileSync(
    path.join(import.meta.dirname, "..", "src", "ble", "browser.rs"),
    "utf8",
  );
  const budget = /rpc\("ble\.open", &\[\], (\d+)\)/.exec(source);
  assert.ok(budget, "ble.open budget not found in src/ble/browser.rs");
  assert.ok(
    Number(budget[1]) >= OPEN_WORST_CASE_MS,
    `ble.open budget ${budget[1]} ms is below open()'s worst case ${OPEN_WORST_CASE_MS} ms`,
  );
});
