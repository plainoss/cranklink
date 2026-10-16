export const FILTER = Object.freeze({ vendorId: 0x1e44, productId: 0x7151 });
export function stripFtdiStatus(bytes, packetSize) {
  if (!Number.isInteger(packetSize) || packetSize < 3)
    throw new Error("Invalid FTDI USB packet size");
  const chunks = [];
  let length = 0;
  for (let i = 0; i < bytes.length; i += packetSize) {
    const packet = bytes.subarray(i, Math.min(i + packetSize, bytes.length));
    if (packet.length < 2) throw new Error("Truncated FTDI status header");
    if (packet[1] & 0x1e)
      throw new Error(`FTDI UART line error 0x${packet[1].toString(16)}`);
    chunks.push(packet.subarray(2));
    length += packet.length - 2;
  }
  const out = new Uint8Array(length);
  let offset = 0;
  for (const c of chunks) {
    out.set(c, offset);
    offset += c.length;
  }
  return out;
}
export class WebUsbPort {
  constructor(device) {
    this.device = device;
    this.opened = false;
    this.queue = [];
    this.queued = 0;
    this.waiters = [];
    this.error = null;
    this.generation = 0;
    this.poisoned = null;
    this.writePending = false;
  }
  describe() {
    const d = this.device;
    return {
      vendorId: d.vendorId,
      productId: d.productId,
      productName: d.productName,
      manufacturerName: d.manufacturerName,
      serialNumber: d.serialNumber,
      deviceVersion: [
        d.deviceVersionMajor,
        d.deviceVersionMinor,
        d.deviceVersionSubminor,
      ],
      configurations: d.configurations.map((c) => ({
        configurationValue: c.configurationValue,
        interfaces: c.interfaces.map((i) => ({
          interfaceNumber: i.interfaceNumber,
          alternates: i.alternates.map((a) => ({
            alternateSetting: a.alternateSetting,
            interfaceClass: a.interfaceClass,
            endpoints: a.endpoints.map((e) => ({
              endpointNumber: e.endpointNumber,
              direction: e.direction,
              type: e.type,
              packetSize: e.packetSize,
            })),
          })),
        })),
      })),
    };
  }
  async control(request, value, index = 0) {
    const r = await this.device.controlTransferOut({
      requestType: "vendor",
      recipient: "device",
      request,
      value,
      index,
    });
    if (r.status !== "ok")
      throw new Error(`FTDI control ${request} failed: ${r.status}`);
  }
  async open(options) {
    if (this.poisoned)
      throw new Error(
        `USB session cannot reopen after an uncertain operation: ${this.poisoned.message}. Reconcile the motor and select a new USB session.`,
      );
    if (this.opened)
      throw new Error("USB port is already owned by another command");
    if (
      options.baudRate !== 2_000_000 ||
      options.dataBits !== 8 ||
      options.parity !== "none" ||
      options.stopBits !== 1 ||
      options.flowControl !== "none" ||
      options.rts !== true
    )
      throw new Error("Unsupported serial configuration");
    const d = this.device;
    if (d.vendorId !== FILTER.vendorId || d.productId !== FILTER.productId)
      throw new Error("Only SM-PCE02 1E44:7151 is supported");
    // The verified control encoding below is for single-port FT232B/R/FT-X.
    if (![4, 6, 0x10].includes(d.deviceVersionMajor))
      throw new Error(
        `FTDI chip revision ${d.deviceVersionMajor} needs a verified baud-rate encoding; no motor command sent`,
      );
    await d.open();
    try {
      if (!d.configuration)
        await d.selectConfiguration(d.configurations[0].configurationValue);
      const choices = d.configuration.interfaces.flatMap((i) =>
        i.alternates
          .filter((a) => a.interfaceClass === 255)
          .map((a) => ({ i, a })),
      );
      const choice = choices.find(
        ({ a }) =>
          a.endpoints.some((e) => e.direction === "in" && e.type === "bulk") &&
          a.endpoints.some((e) => e.direction === "out" && e.type === "bulk"),
      );
      if (!choice) throw new Error("No vendor bulk serial interface");
      this.interfaceNumber = choice.i.interfaceNumber;
      if (this.interfaceNumber !== 0)
        throw new Error("Only the single-port FTDI interface 0 is supported");
      try {
        await d.claimInterface(this.interfaceNumber);
      } catch (e) {
        throw new Error(
          `Cannot claim SM-PCE02 USB interface: ${e.message}. Windows needs the USB function bound to WinUSB; close E-TUBE and COM-port tools.`,
        );
      }
      if (choice.a.alternateSetting !== 0)
        await d.selectAlternateInterface(
          this.interfaceNumber,
          choice.a.alternateSetting,
        );
      this.input = choice.a.endpoints.find(
        (e) => e.direction === "in" && e.type === "bulk",
      );
      this.output = choice.a.endpoints.find(
        (e) => e.direction === "out" && e.type === "bulk",
      );
      await this.control(0, 0);
      await this.control(0, 1);
      await this.control(0, 2);
      await this.control(4, 8);
      await this.control(2, 0);
      await this.control(3, 1);
      await this.control(1, 0x0202);
      await this.control(9, 2);
      this.queue = [];
      this.queued = 0;
      this.error = null;
      this.opened = true;
      const generation = ++this.generation;
      this.pumpPromise = this.pump(generation);
    } catch (e) {
      await d.close().catch(() => {});
      throw e;
    }
  }
  async pump(generation) {
    try {
      while (this.opened && generation === this.generation) {
        // Exactly one transferIn is outstanding, even when a Rust read times out.
        const r = await this.device.transferIn(
          this.input.endpointNumber,
          this.input.packetSize * 64,
        );
        if (!this.opened || generation !== this.generation) break;
        if (r.status !== "ok") throw new Error(`USB receive ${r.status}`);
        const data = stripFtdiStatus(
          new Uint8Array(r.data.buffer, r.data.byteOffset, r.data.byteLength),
          this.input.packetSize,
        );
        if (data.length) {
          this.queue.push(data.slice());
          this.queued += data.length;
          if (this.queued > 1024 * 1024)
            throw new Error("USB receive queue exceeded 1 MiB");
          this.wake();
        }
      }
    } catch (e) {
      if (this.opened && generation === this.generation) {
        this.error = e;
        this.poisoned = e;
        this.wake();
        void this.close().catch(() => {});
      }
    }
  }
  wake() {
    for (const resolve of this.waiters.splice(0)) resolve();
  }
  async read(max, timeout) {
    if (this.error) throw this.error;
    if (!this.opened) throw new Error("USB port is closed");
    const deadline = performance.now() + timeout;
    while (!this.queued && !this.error && this.opened) {
      const remaining = deadline - performance.now();
      if (remaining <= 0) return new Uint8Array();
      await new Promise((resolve) => {
        const wake = () => {
          clearTimeout(timer);
          resolve();
        };
        const timer = setTimeout(() => {
          this.waiters = this.waiters.filter((x) => x !== wake);
          resolve();
        }, remaining);
        this.waiters.push(wake);
      });
    }
    if (this.error) throw this.error;
    if (!this.opened) throw new Error("USB disconnected");
    const out = new Uint8Array(Math.min(max, this.queued));
    let offset = 0;
    while (offset < out.length) {
      const first = this.queue[0],
        take = Math.min(first.length, out.length - offset);
      out.set(first.subarray(0, take), offset);
      offset += take;
      this.queued -= take;
      if (take === first.length) this.queue.shift();
      else this.queue[0] = first.subarray(take);
    }
    return out;
  }
  async write(bytes, timeout = 500) {
    if (this.poisoned) throw this.poisoned;
    if (!this.opened) throw new Error("USB port is closed");
    if (this.error) throw this.error;
    if (this.writePending) throw new Error("A USB write is already pending");
    if (!Number.isFinite(timeout) || timeout < 0 || timeout > 0x7fffffff)
      throw new Error("Invalid USB write timeout");
    this.writePending = true;
    const generation = this.generation;
    let timer;
    try {
      const transfer = Promise.resolve().then(() =>
        this.device.transferOut(this.output.endpointNumber, bytes),
      );
      const deadline = new Promise((_, reject) => {
        timer = setTimeout(
          () =>
            reject(
              new Error(
                `USB write timed out after ${timeout} ms; device outcome uncertain`,
              ),
            ),
          timeout,
        );
      });
      const r = await Promise.race([transfer, deadline]);
      if (generation !== this.generation || !this.opened || this.poisoned)
        throw new Error(
          "USB session changed during write; device outcome uncertain",
        );
      if (r.status !== "ok" || r.bytesWritten !== bytes.length)
        throw new Error(
          `USB write incomplete (${r.bytesWritten}/${bytes.length}); device outcome uncertain`,
        );
    } catch (cause) {
      // A rejected/expired USB transfer may already have reached the motor.
      // Closing cannot roll it back, and this object must never dispatch again.
      const detail = cause?.message || String(cause);
      const error = new Error(
        detail.includes("uncertain")
          ? detail
          : `USB write failed: ${detail}; device outcome uncertain`,
      );
      this.poisoned = error;
      this.error = error;
      this.wake();
      void this.close().catch(() => {});
      throw error;
    } finally {
      clearTimeout(timer);
      this.writePending = false;
    }
  }
  disconnected() {
    const error = new Error(
      "USB device disconnected; device outcome may be uncertain",
    );
    this.error = error;
    this.poisoned = error;
    this.wake();
    void this.close().catch(() => {});
  }
  async close() {
    if (!this.opened && !this.device.opened) return;
    this.opened = false;
    ++this.generation;
    this.wake();
    await this.device.close();
    this.queue = [];
    this.queued = 0;
  }
}
