// GATT only: authentication, command encoding and settings policy live in Rust.
export const ETUBE_SERVICE = "000018ff-5348-494d-414e-4f5f424c4500";
export const characteristicUuid = (id) =>
  `0000${id.toString(16).padStart(4, "0")}-5348-494d-414e-4f5f424c4500`;
const IDS = [0x2af3, 0x2af4, 0x2af9, 0x2afa, 0x2afd, 0x2afe, 0x2aff];
const RESPONSES = new Set([0x2af3, 0x2af9, 0x2afd]);
const enc = new TextEncoder();
const bytesOf = (view) =>
  new Uint8Array(view.buffer, view.byteOffset, view.byteLength).slice();

export class WebBleGatt {
  static choose(bluetooth = globalThis.navigator?.bluetooth) {
    if (!bluetooth || !globalThis.isSecureContext)
      throw new Error(
        "Web Bluetooth requires a supported browser in a secure context. USB and Serial remain available.",
      );
    // Chooser is invoked in the user action; no background discovery/pairing.
    return bluetooth
      .requestDevice({
        filters: [{ services: [ETUBE_SERVICE] }],
        optionalServices: [ETUBE_SERVICE],
      })
      .catch((err) => {
        if (
          err &&
          typeof err.message === "string" &&
          /globally disabled/i.test(err.message)
        ) {
          throw new Error(
            "Web Bluetooth is disabled globally in this browser. Use Google Chrome or Microsoft Edge to connect wirelessly, or enable brave://flags/#enable-web-bluetooth in Brave.",
          );
        }
        throw err;
      });
  }
  constructor(device, requestPasskey) {
    this.device = device;
    this.requestPasskey = requestPasskey;
    this.chars = new Map();
    this.handlers = new Map();
    this.queue = [];
    this.pending = null;
    this.epoch = 0;
    this.opened = false;
    this.failure = null;
    this.credential = null;
    this.credentialPrompt = null;
    this.onDisconnect = () => {
      if (this.opened)
        this.fail(
          new Error("Bluetooth disconnected; reconnect before another command"),
        );
    };
    device.addEventListener("gattserverdisconnected", this.onDisconnect);
  }
  describe() {
    return {
      transport: "ble",
      id: this.device.id,
      name: this.device.name,
      hardwareValidated: false,
      firmwareUpdates: "experimental EP800 motor relay (--ble-firmware)",
      firmwareHardwareValidated: false,
      radioDfu: false,
    };
  }
  clearConnection(reason) {
    this.credentialPrompt?.abort();
    this.credentialPrompt = null;
    this.epoch++;
    this.opened = false;
    for (const [id, handler] of this.handlers)
      this.chars
        .get(id)
        ?.removeEventListener("characteristicvaluechanged", handler);
    this.handlers.clear();
    this.chars.clear();
    this.queue.length = 0;
    if (this.pending) {
      const p = this.pending;
      this.pending = null;
      clearTimeout(p.timer);
      p.reject(reason);
    }
  }
  fail(error) {
    // Preserve credential in memory so user is not prompted for the passkey again
    // on subsequent operations. Credential is cleared on explicit forget() / disconnect.
    this.failure = error;
    this.clearConnection(error);
    if (this.device.gatt?.connected) this.device.gatt.disconnect();
  }
  async close() {
    this.clearConnection(new Error("Bluetooth connection closed"));
    if (this.device.gatt?.connected) this.device.gatt.disconnect();
  }
  async forget() {
    this.credential = null;
    await this.close();
    this.device.removeEventListener(
      "gattserverdisconnected",
      this.onDisconnect,
    );
  }
  async bounded(action, milliseconds = 10000) {
    const epoch = this.epoch;
    let timer;
    try {
      return await Promise.race([
        Promise.resolve()
          .then(action)
          .then((value) => {
            if (this.epoch !== epoch) {
              // A timed-out connect may complete later. Close it, never revive it.
              if (this.device.gatt?.connected) this.device.gatt.disconnect();
              throw new Error(
                "Stale Bluetooth operation completed after disconnect",
              );
            }
            return value;
          }),
        new Promise((_, reject) => {
          timer = setTimeout(
            () =>
              reject(
                new Error(
                  "Bluetooth operation timed out; outcome may be uncertain",
                ),
              ),
            milliseconds,
          );
        }),
      ]);
    } catch (error) {
      this.fail(error);
      throw error;
    } finally {
      clearTimeout(timer);
    }
  }
  async open() {
    await this.close();
    this.failure = null;
    let server = null;
    let lastError = null;
    // Shimano displays (e.g. SC-EM800) take a moment to resume advertising after
    // acknowledging a PC-link stop from a previous command. Retry connect with backoff.
    for (let attempt = 1; attempt <= 3; attempt++) {
      try {
        server = await this.bounded(() => this.device.gatt.connect(), 12000);
        break;
      } catch (err) {
        lastError = err;
        if (attempt < 3) {
          await new Promise((r) => setTimeout(r, 800 * attempt));
        }
      }
    }
    if (!server) {
      const err = lastError || new Error("Failed to connect to Bluetooth device");
      this.fail(err);
      throw err;
    }
    const service = await this.bounded(() =>
      server.getPrimaryService(ETUBE_SERVICE),
    );
    const characteristics = await this.bounded(() =>
      service.getCharacteristics(),
    );
    for (const id of IDS) {
      const c = characteristics.find(
        (c) => c.uuid.toLowerCase() === characteristicUuid(id),
      );
      if (c) this.chars.set(id, c);
    }
    if (
      ![0x2af9, 0x2afa, 0x2afd, 0x2afe, 0x2aff].every((id) =>
        this.chars.has(id),
      )
    ) {
      const error = new Error(
        "Device has no supported E-TUBE configuration service",
      );
      this.fail(error);
      throw error;
    }
    this.opened = true;
    return [...this.chars].map(([id, c]) => ({
      id,
      read: c.properties.read,
      write: c.properties.write,
      write_without_response: c.properties.writeWithoutResponse,
      notify: c.properties.notify,
      indicate: c.properties.indicate,
    }));
  }
  characteristic(id) {
    if (!this.opened || !this.device.gatt.connected || this.failure)
      throw this.failure || new Error("Bluetooth connection is closed");
    const c = this.chars.get(id);
    if (!c) throw new Error("Unsupported or unavailable BLE characteristic");
    return c;
  }
  notification(id, view) {
    if (!this.opened || this.failure) return;
    const value = bytesOf(view);
    if (value.length > 20 || this.queue.length >= 256) {
      this.fail(
        new Error(
          "Bluetooth notification exceeded the supported size/queue bound",
        ),
      );
      return;
    }
    const n = { characteristic: id, value: [...value] };
    if (this.pending) {
      const p = this.pending;
      this.pending = null;
      clearTimeout(p.timer);
      p.resolve(n);
    } else this.queue.push(n);
  }
  async subscribe(id) {
    if (!RESPONSES.has(id))
      throw new Error("BLE boot/DFU subscriptions are disabled");
    const c = this.characteristic(id);
    if (this.handlers.has(id)) return;
    const handler = (event) => this.notification(id, event.target.value);
    // Attach before enabling to retain notifications arriving during the call.
    this.handlers.set(id, handler);
    c.addEventListener("characteristicvaluechanged", handler);
    await this.bounded(() => c.startNotifications());
  }
  receive(timeout) {
    this.characteristic(0x2af9);
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    if (this.pending)
      throw new Error("Concurrent BLE notification reads are disabled");
    if (!timeout) return Promise.resolve(null);
    return new Promise((resolve, reject) => {
      const pending = { resolve, reject, timer: null };
      this.pending = pending;
      pending.timer = setTimeout(
        () => {
          if (this.pending !== pending) return;
          this.pending = null;
          resolve(null);
        },
        Math.min(timeout, 5000),
      );
    });
  }
  async request(op, payload, timeout) {
    const p = payload.length
      ? JSON.parse(new TextDecoder().decode(payload))
      : {};
    if (op === "ble.open") return enc.encode(JSON.stringify(await this.open()));
    if (op === "ble.close") {
      await this.close();
      return new Uint8Array();
    }
    if (op === "ble.passkey") {
      if (!this.opened) throw new Error("Bluetooth is closed");
      if (!this.credential) {
        const epoch = this.epoch;
        const controller = new AbortController();
        this.credentialPrompt = controller;
        try {
          const value = await this.requestPasskey(controller.signal);
          if (!this.opened || epoch !== this.epoch)
            throw new Error("Bluetooth closed while entering passkey");
          this.credential = value;
        } finally {
          if (this.credentialPrompt === controller)
            this.credentialPrompt = null;
        }
      }
      if (!/^[0-9]{6}$/.test(this.credential || "")) {
        this.credential = null;
        throw new Error("BLE passkey must be six ASCII digits");
      }
      return enc.encode(this.credential);
    }
    if (op === "ble.receive")
      return enc.encode(JSON.stringify(await this.receive(timeout)));
    if (!IDS.includes(p.id))
      throw new Error("BLE boot/DFU operations are disabled");
    if (op === "ble.subscribe") {
      await this.subscribe(p.id);
      return new Uint8Array();
    }
    const c = this.characteristic(p.id);
    if (op === "ble.read")
      return bytesOf(await this.bounded(() => c.readValue()));
    if (op === "ble.write") {
      if (
        ![0x2af3, 0x2afa, 0x2afe, 0x2aff].includes(p.id) ||
        !Array.isArray(p.bytes) ||
        p.bytes.length > 20 ||
        !p.bytes.every((x) => Number.isInteger(x) && x >= 0 && x <= 255)
      )
        throw new Error("Unsupported BLE write");
      if (
        p.id === 0x2aff &&
        (p.bytes.length !== 2 || p.bytes[0] !== 255 || p.bytes[1] !== 0)
      )
        throw new Error("BLE mode switching/DFU is disabled");
      const value = Uint8Array.from(p.bytes);
      if (p.withoutResponse) {
        if (!c.properties.writeWithoutResponse)
          throw new Error("BLE write without response unavailable");
        await this.bounded(() => c.writeValueWithoutResponse(value));
      } else {
        if (!c.properties.write)
          throw new Error("BLE write with response unavailable");
        await this.bounded(() => c.writeValueWithResponse(value));
      }
      return new Uint8Array();
    }
    throw new Error("Unsupported Bluetooth host operation");
  }
}
