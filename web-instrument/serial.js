// Raw Web Serial byte transport for the same SM-PCE02 framing used by Rust.
// API lifecycle: https://developer.chrome.com/docs/capabilities/serial
export const SERIAL_FILTER = Object.freeze({
  usbVendorId: 0x1e44,
  usbProductId: 0x7151,
});
const QUEUE_LIMIT = 1024 * 1024;
export function assertSupportedSerial(port) {
  const info = port.getInfo();
  if (
    info.usbVendorId !== SERIAL_FILTER.usbVendorId ||
    info.usbProductId !== SERIAL_FILTER.usbProductId
  )
    throw new Error(
      "Only the SM-PCE02 serial port 1E44:7151 is supported; no motor command sent",
    );
  return info;
}
export function chooseTransport(
  requested = "auto",
  environment = globalThis.navigator,
) {
  if (!["auto", "usb", "serial"].includes(requested))
    throw new Error("Usage: connect [auto|usb|serial]");
  const windows = /Windows|^Win/i.test(
    environment.userAgentData?.platform ||
      environment.platform ||
      environment.userAgent ||
      "",
  );
  const selected =
    requested === "auto"
      ? windows && environment.serial
        ? "serial"
        : "usb"
      : requested;
  if (!environment[selected])
    throw new Error(
      selected === "serial"
        ? "Web Serial is unavailable. Use desktop Chrome/Edge with a serial driver, or run connect usb for direct USB."
        : "WebUSB is unavailable. Use desktop Chrome/Edge in a secure context, or run connect serial with a serial driver.",
    );
  return selected;
}
export async function requestAdapter(
  requested = "auto",
  environment = globalThis.navigator,
) {
  const transport = chooseTransport(requested, environment);
  if (transport === "serial") {
    const selected = await environment.serial.requestPort({
      filters: [SERIAL_FILTER],
    });
    assertSupportedSerial(selected);
    return { transport, selected };
  }
  const selected = await environment.usb.requestDevice({
    filters: [
      {
        vendorId: SERIAL_FILTER.usbVendorId,
        productId: SERIAL_FILTER.usbProductId,
      },
    ],
  });
  if (
    selected.vendorId !== SERIAL_FILTER.usbVendorId ||
    selected.productId !== SERIAL_FILTER.usbProductId
  )
    throw new Error("Only SM-PCE02 USB device 1E44:7151 is supported");
  return { transport, selected };
}

function bounded(promise, milliseconds, message) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(message)), milliseconds);
    }),
  ]).finally(() => clearTimeout(timer));
}
export class WebSerialPort {
  constructor(serialPort, { closeTimeout = 1000 } = {}) {
    assertSupportedSerial(serialPort);
    this.serialPort = serialPort;
    this.transport = "serial";
    this.closeTimeout = closeTimeout;
    this.opened = false;
    this.opening = false;
    this.resources = null;
    this.closePromise = null;
    this.queue = [];
    this.queued = 0;
    this.waiters = [];
    this.generation = 0;
    this.error = null;
    this.poisoned = null;
    this.writePending = false;
  }
  describe() {
    return { transport: "serial", ...assertSupportedSerial(this.serialPort) };
  }
  async open(options) {
    if (this.poisoned)
      throw new Error(
        `Serial session cannot reopen after an uncertain operation: ${this.poisoned.message}. Reconcile the motor and select a new serial session.`,
      );
    if (this.opened || this.opening || this.closePromise)
      throw new Error("Serial port is already owned by another command");
    assertSupportedSerial(this.serialPort);
    if (
      options.baudRate !== 2_000_000 ||
      options.dataBits !== 8 ||
      options.stopBits !== 1 ||
      options.parity !== "none" ||
      options.flowControl !== "none" ||
      options.rts !== true
    )
      throw new Error("Unsupported serial configuration");
    const generation = ++this.generation;
    this.opening = true;
    let acquired = false;
    try {
      await this.serialPort.open({
        baudRate: 2_000_000,
        dataBits: 8,
        stopBits: 1,
        parity: "none",
        flowControl: "none",
        bufferSize: 65536,
      });
      acquired = true;
      if (generation !== this.generation)
        throw new Error("Serial open canceled; select a new session");
      this.resources = { reader: null, writer: null, pump: null };
      await this.serialPort.setSignals({ requestToSend: true });
      if (generation !== this.generation)
        throw new Error("Serial open canceled; select a new session");
      if (!this.serialPort.readable || !this.serialPort.writable)
        throw new Error(
          "Serial streams are unavailable; close E-TUBE and other COM-port tools",
        );
      const resources = this.resources;
      resources.reader = this.serialPort.readable.getReader();
      resources.writer = this.serialPort.writable.getWriter();
      this.queue = [];
      this.queued = 0;
      this.error = null;
      this.opened = true;
      resources.pump = this.pump(resources, generation);
    } catch (error) {
      if (acquired) {
        // If close raced a pending native open, the late acquired port must also
        // be closed. It cannot become a usable new session in this generation.
        if (!this.resources)
          this.resources = { reader: null, writer: null, pump: null };
        await this.close().catch(() => {});
      }
      throw error;
    } finally {
      this.opening = false;
    }
  }
  async pump(resources, generation) {
    try {
      while (this.opened && generation === this.generation) {
        // Rust read timeouts never cancel this stream read or start a second.
        const { value, done } = await resources.reader.read();
        if (!this.opened || generation !== this.generation) break;
        if (done) throw new Error("Serial device disconnected");
        if (value?.length) {
          // Web Serial has already removed USB/FTDI status framing. Keep bytes raw.
          if (this.queued + value.length > QUEUE_LIMIT)
            throw new Error("Serial receive queue exceeded 1 MiB");
          const bytes = Uint8Array.from(value);
          this.queue.push(bytes);
          this.queued += bytes.length;
          this.wake();
        }
      }
    } catch (error) {
      if (this.opened && generation === this.generation) {
        this.error = error;
        this.poisoned = error;
        this.wake();
        void this.close().catch(() => {});
      }
    } finally {
      try {
        resources.reader.releaseLock();
      } catch {}
    }
  }
  wake() {
    for (const resolve of this.waiters.splice(0)) resolve();
  }
  async read(max, timeout) {
    if (this.error) throw this.error;
    if (!this.opened) throw new Error("Serial port is closed");
    if (
      !Number.isInteger(max) ||
      max < 0 ||
      !Number.isFinite(timeout) ||
      timeout < 0 ||
      timeout > 0x7fffffff
    )
      throw new Error("Invalid serial read bounds");
    const generation = this.generation,
      deadline = performance.now() + timeout;
    while (
      !this.queued &&
      !this.error &&
      this.opened &&
      generation === this.generation
    ) {
      const remaining = deadline - performance.now();
      if (remaining <= 0) return new Uint8Array();
      await new Promise((resolve) => {
        const wake = () => {
          clearTimeout(timer);
          resolve();
        };
        const timer = setTimeout(() => {
          this.waiters = this.waiters.filter((r) => r !== wake);
          resolve();
        }, remaining);
        this.waiters.push(wake);
      });
    }
    if (this.error) throw this.error;
    if (!this.opened || generation !== this.generation)
      throw new Error("Serial device disconnected");
    const out = new Uint8Array(Math.min(max, this.queued));
    let offset = 0;
    while (offset < out.length) {
      const chunk = this.queue[0],
        take = Math.min(chunk.length, out.length - offset);
      out.set(chunk.subarray(0, take), offset);
      offset += take;
      this.queued -= take;
      if (take === chunk.length) this.queue.shift();
      else this.queue[0] = chunk.subarray(take);
    }
    return out;
  }
  async write(bytes, timeout = 500) {
    if (this.poisoned) throw this.poisoned;
    if (this.error) throw this.error;
    if (!this.opened) throw new Error("Serial port is closed");
    if (this.writePending) throw new Error("A serial write is already pending");
    if (!Number.isFinite(timeout) || timeout < 0 || timeout > 0x7fffffff)
      throw new Error("Invalid serial write timeout");
    this.writePending = true;
    const generation = this.generation,
      writer = this.resources.writer;
    try {
      const result = await bounded(
        Promise.resolve().then(() => writer.write(Uint8Array.from(bytes))),
        timeout,
        `Serial write timed out after ${timeout} ms; device outcome uncertain`,
      );
      if (generation !== this.generation || !this.opened)
        throw new Error(
          "Serial session changed during write; device outcome uncertain",
        );
      // A Web Serial WritableStream write resolves with undefined, never a
      // partial byte count. Reject nonstandard partial receipts rather than
      // treating them as complete, and never retry a rejected/expired write.
      if (result !== undefined)
        throw new Error(
          "Serial write returned an incomplete or unsupported receipt; device outcome uncertain",
        );
    } catch (cause) {
      const detail = cause?.message || String(cause);
      const error = new Error(
        detail.includes("uncertain")
          ? detail
          : `Serial write failed: ${detail}; device outcome uncertain`,
      );
      this.poisoned = error;
      this.error = error;
      this.wake();
      void this.close().catch(() => {});
      throw error;
    } finally {
      this.writePending = false;
    }
  }
  disconnected() {
    const error = new Error(
      "Serial device disconnected; device outcome may be uncertain",
    );
    this.error = error;
    this.poisoned = error;
    this.wake();
    void this.close().catch(() => {});
  }
  async close() {
    if (this.closePromise) return this.closePromise;
    if (!this.resources && !this.opening) {
      this.opened = false;
      this.wake();
      return;
    }
    this.opened = false;
    ++this.generation;
    this.wake();
    this.queue = [];
    this.queued = 0;
    const resources = this.resources;
    if (!resources) {
      // A pending native open cannot be canceled through Web Serial. Fence its
      // late result; open() will close it on completion and this wrapper cannot reopen.
      const error = new Error(
        "Serial close interrupted a pending open; session outcome uncertain",
      );
      this.poisoned = error;
      this.error = error;
      return;
    }
    const cleanup = (async () => {
      try {
        await resources.reader?.cancel();
      } catch {}
      try {
        await resources.pump;
      } catch {}
      try {
        resources.reader?.releaseLock();
      } catch {}
      if (this.writePending || this.poisoned) {
        try {
          await resources.writer?.abort(
            this.poisoned || new Error("Serial session closed"),
          );
        } catch {}
      }
      try {
        resources.writer?.releaseLock();
      } catch {}
      await this.serialPort.close();
      if (this.resources === resources) this.resources = null;
    })();
    this.closePromise = (async () => {
      try {
        await bounded(
          cleanup,
          this.closeTimeout,
          `Serial close timed out after ${this.closeTimeout} ms; session outcome uncertain`,
        );
      } catch (cause) {
        const error = cause instanceof Error ? cause : new Error(String(cause));
        this.poisoned = this.poisoned || error;
        this.error = this.error || error;
        throw error;
      } finally {
        if (!this.resources) this.closePromise = null;
      }
    })();
    return this.closePromise;
  }
}
