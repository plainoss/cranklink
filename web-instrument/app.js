import { FILTER, WebUsbPort } from "./usb.js";
import { WebBleGatt } from "./ble.js";
import { BrowserFiles } from "./files.js";
import {
  WebSerialPort,
  chooseTransport,
  requestAdapter,
  assertSupportedSerial,
} from "./serial.js";
const DeviceSimulator = import.meta.env.DEV
  ? (await import("./simulator.js")).DeviceSimulator
  : null;
const output = document.querySelector("#output"),
  form = document.querySelector("#command-form"),
  input = document.querySelector("#command");
const picker = document.querySelector("#file-picker");
const files = new BrowserFiles();
let selectedTransport = null;
let activeSimulator = null;
let activeSimulatorOptions = {};
let pendingBleDisconnect = null;
let releaseBleSessionLock = null;
let port,
  worker,
  busy = false,
  ready = false,
  current,
  history = [],
  historyIndex = 0;
const encoder = new TextEncoder(),
  decoder = new TextDecoder();
let shared, control, buffer;
function setSimulatorActive(active) {
  if (shared?.simulator) {
    Atomics.store(new Int32Array(shared.simulator), 0, active ? 1 : 0);
  }
}
function print(text, stream = "stdout") {
  const span = document.createElement("span");
  span.textContent = text;
  if (stream === "stderr") span.className = "error";
  output?.append(span);
  while (output && output.childNodes.length > 10000) output.firstChild.remove();
  if (output) output.scrollTop = output.scrollHeight;
  if (typeof window !== "undefined" && window.estepsTerminalPrint) {
    window.estepsTerminalPrint(text, stream);
  }
}
function args(line) {
  const result = [];
  let word = "",
    quote = "",
    escaped = false,
    started = false;
  for (const c of line) {
    if (escaped) {
      word += c;
      escaped = false;
      started = true;
      continue;
    }
    if (c === "\\" && quote !== "'") {
      escaped = true;
      started = true;
      continue;
    }
    if (quote) {
      if (c === quote) quote = "";
      else word += c;
      continue;
    }
    if (c === '"' || c === "'") {
      quote = c;
      started = true;
      continue;
    }
    if (/\s/.test(c)) {
      if (started) {
        result.push(word);
        word = "";
        started = false;
      }
    } else {
      word += c;
      started = true;
    }
  }
  if (quote || escaped) throw new Error("Unclosed quote or trailing escape");
  if (started) result.push(word);
  return result;
}
function download(name, bytes) {
  const url = URL.createObjectURL(new Blob([bytes]));
  const a = document.createElement("a");
  a.href = url;
  a.download = name.split("/").pop();
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
async function rpc(message) {
  let response,
    error = 0;
  try {
    if (message.op.startsWith("fs."))
      response = await files.request(
        message.op,
        JSON.parse(decoder.decode(message.payload)),
      );
    else if (message.op.startsWith("ble.")) {
      if (
        !(port instanceof WebBleGatt) ||
        (selectedTransport !== "ble" && selectedTransport !== "sim")
      )
        throw new Error("No Bluetooth device selected. Run connect ble first.");
      response = await port.request(
        message.op,
        message.payload,
        message.timeout,
      );
    } else if (
      message.op.startsWith("io.") &&
      (selectedTransport === "ble" || selectedTransport === "sim")
    ) {
      throw new Error(
        "USB/Serial commands cannot use the selected Bluetooth device",
      );
    } else if (message.op === "io.open") {
      if (!port) throw new Error("No adapter selected. Run connect first.");
      const options = JSON.parse(decoder.decode(message.payload));
      const expected = selectedTransport === "serial" ? "webserial" : "webusb";
      if (options.port !== expected && options.port !== "auto")
        throw new Error(
          `Selected ${selectedTransport} transport requires --port ${expected}; run connect usb or connect serial to change it`,
        );
      await port.open(options);
      response = new Uint8Array();
    } else if (message.op === "io.close") {
      if (port) await port.close();
      response = new Uint8Array();
    } else if (message.op === "io.write") {
      if (!port) throw new Error("No adapter selected");
      await port.write(message.payload, message.timeout);
      response = new Uint8Array();
    } else if (message.op === "io.read") {
      if (!port) throw new Error("No adapter selected");
      response = await port.read(
        new DataView(
          message.payload.buffer,
          message.payload.byteOffset,
          4,
        ).getUint32(0, true),
        message.timeout,
      );
    } else throw new Error(`Unsupported host operation ${message.op}`);
  } catch (e) {
    error = 1;
    response = encoder.encode(e.message || String(e));
  }
  if (response.length > buffer.length) {
    error = 1;
    response = encoder.encode("Host response exceeds shared buffer");
  }
  // A late result must never satisfy a newer command after a host timeout.
  if (
    Atomics.load(control, 3) !== message.id ||
    Atomics.compareExchange(control, 0, 0, 3) !== 0
  )
    return;
  buffer.set(response);
  Atomics.store(control, 2, response.length);
  Atomics.store(control, 1, error);
  Atomics.store(control, 0, 1);
  Atomics.notify(control, 0);
}
async function complete(code, error) {
  try {
    if (error && port) await port.close();
    await files.syncAll();
  } catch (e) {
    code = 1;
    print(`Persistence failed: ${e.message}\n`, "stderr");
  }
  if (error) print(`${error}\n`, "stderr");
  busy = false;
  if (input) {
    input.disabled = false;
    input.focus();
  }
  if (code) print(`[exit ${code}]\n`, "stderr");
  current?.resolve({ code });
  current = null;
}
function startWorker() {
  worker = new Worker("./worker.js", { type: "module" });
  worker.onmessage = (event) => {
    const m = event.data;
    if (m.type === "rpc") {
      rpc(m);
      return;
    }
    if (m.type === "output") {
      print(m.text, m.stream);
      return;
    }
    if (m.type === "ready") {
      ready = true;
      print("Rust WASM ready. Type help.\n");
      return;
    }
    if (m.type === "ble-disconnected") {
      const pending = pendingBleDisconnect;
      pendingBleDisconnect = null;
      if (m.code) pending?.reject(new Error(m.error || "Bluetooth cleanup failed; check the bike before reconnecting."));
      else pending?.resolve();
      return;
    }
    if (m.type === "host-failed") {
      pendingBleDisconnect?.reject(new Error("Browser host failed during Bluetooth cleanup."));
      pendingBleDisconnect = null;
      ready = false;
      port?.close().catch(() => {});
      print(
        "Browser host failed; reload before another command. Device outcome may be uncertain.\n",
        "stderr",
      );
      return;
    }
    if (m.type === "done") complete(m.code);
    if (m.type === "fatal") {
      pendingBleDisconnect?.reject(new Error(m.error));
      pendingBleDisconnect = null;
      ready = false;
      complete(1, m.error);
    }
  };
  worker.onerror = (e) => {
    pendingBleDisconnect?.reject(new Error(e.message));
    pendingBleDisconnect = null;
    ready = false;
    complete(1, e.message);
  };
  worker.postMessage({ type: "init", shared });
}
let cachedBlePasskey = null;
try {
  cachedBlePasskey = sessionStorage.getItem("esteps_ble_passkey");
} catch {}

function requestBlePasskey(signal) {
  if (cachedBlePasskey && /^[0-9]{6}$/.test(cachedBlePasskey)) {
    return Promise.resolve(cachedBlePasskey);
  }
  const dialog = document.querySelector("#ble-passkey-dialog");
  const field = document.querySelector("#ble-passkey");
  const cancel = document.querySelector("#ble-passkey-cancel");
  if (dialog.open) throw new Error("A passkey prompt is already open");
  field.value = "";
  dialog.returnValue = "";
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => dialog.close("timeout"), 120000);
    const onCancel = () => dialog.close("cancel");
    const onAbort = () => dialog.close("cancel");
    const close = () => {
      clearTimeout(timer);
      cancel.removeEventListener("click", onCancel);
      signal?.removeEventListener("abort", onAbort);
      const value = field.value;
      field.value = "";
      if (dialog.returnValue === "authenticate" && /^[0-9]{6}$/.test(value)) {
        cachedBlePasskey = value;
        try {
          sessionStorage.setItem("esteps_ble_passkey", value);
        } catch {}
        resolve(value);
      } else {
        reject(new Error("Bluetooth authentication cancelled or timed out"));
      }
    };
    cancel.addEventListener("click", onCancel);
    signal?.addEventListener("abort", onAbort, { once: true });
    dialog.addEventListener("close", close, { once: true });
    dialog.showModal();
    field.focus();
    if (signal?.aborted) dialog.close("cancel");
  });
}
function clearBlePasskey() {
  cachedBlePasskey = null;
  try {
    sessionStorage.removeItem("esteps_ble_passkey");
  } catch {}
}
async function withDeviceLock(action) {
  if (busy) throw new Error("A command is already running");
  busy = true;
  if (input) input.disabled = true;
  try {
    if (!navigator.locks) throw new Error("Browser session locks are required");
    return await navigator.locks.request(
      "esteps-device-command",
      { ifAvailable: true },
      async (lock) => {
        if (!lock)
          throw new Error("Another terminal tab owns the device command");
        return action();
      },
    );
  } finally {
    busy = false;
    if (input) {
      input.disabled = false;
      input.focus();
    }
  }
}
async function adoptPort(next, transport) {
  if (port) {
    await releaseSelectedPort();
  }
  if (transport === "ble") {
    await new Promise((resolve, reject) => {
      navigator.locks.request("cranklink-ble-session", { ifAvailable: true }, async (lock) => {
        if (!lock) {
          reject(new Error("Another tab has an active Bluetooth session. Disconnect it first."));
          return;
        }
        await new Promise((release) => {
          releaseBleSessionLock = release;
          resolve();
        });
      }).catch(reject);
    });
  }
  port = next;
  selectedTransport = transport;
  return { transport, ...port.describe() };
}
async function releaseSelectedPort() {
  try {
    if (ready) {
      await new Promise((resolve, reject) => {
        pendingBleDisconnect = { resolve, reject };
        worker.postMessage({ type: "ble-disconnect" });
      });
    }
  } finally {
    try {
      if (port) {
        await port.close();
        await port.forget?.();
      }
    } finally {
      releaseBleSessionLock?.();
      releaseBleSessionLock = null;
    }
  }
}
async function selectGranted(requested = "auto") {
  return withDeviceLock(async () => {
    const transport = chooseTransport(requested);
    if (transport === "serial") {
      const ports = (await navigator.serial.getPorts()).filter((p) => {
        try {
          assertSupportedSerial(p);
          return true;
        } catch {
          return false;
        }
      });
      if (!ports.length)
        throw new Error(
          "No previously authorized SM-PCE02 serial port. Run connect serial, or connect usb for direct USB.",
        );
      if (ports.length !== 1)
        throw new Error(
          "Multiple authorized SM-PCE02 serial ports. Run connect serial to choose one explicitly.",
        );
      return adoptPort(new WebSerialPort(ports[0]), transport);
    }
    const devices = (await navigator.usb.getDevices()).filter(
      (d) => d.vendorId === FILTER.vendorId && d.productId === FILTER.productId,
    );
    if (!devices.length)
      throw new Error(
        "No previously authorized SM-PCE02 USB device. Run connect usb.",
      );
    if (devices.length !== 1)
      throw new Error(
        "Multiple authorized SM-PCE02 USB devices. Run connect usb to choose one explicitly.",
      );
    return adoptPort(new WebUsbPort(devices[0]), transport);
  });
}

async function run(line) {
  if (busy) throw new Error("A command is already running");
  const tokens = args(line);
  if (!tokens.length) return { code: 0 };
  history.push(line);
  historyIndex = history.length;
  print(`cranklink> ${line}\n`);
  const command = tokens[0];
  if (command === "help") {
    print(
      "Browser commands: connect [auto|usb|serial|ble|sim [model]], disconnect, usb-info, ble-info, import, files, download FILE, cat FILE, clear\nRust commands: ble authenticate, ble units, probe, boot-probe, rx-probe, inspect FILE, flash-dcas, flash-rx, power-cycle, settings ...\nUse --help after any Rust command. Firmware, backups, traces and journals live in durable browser storage.\nConnect auto prefers Web Serial on Windows; other platforms use WebUSB. Bluetooth is experimental and explicit: connect ble. Simulator mode: connect sim [model]. The selected transport sets --port webserial, webusb or webble. Supported EP800 motor firmware relay requires --ble-firmware; BLE recovery and radio DFU are unavailable. Connecting and reading settings do not enter firmware mode.\n",
    );
    return { code: 0 };
  }
  if (command === "connect") {
    const isSim = tokens[1] === "sim" || tokens[1] === "simulator";
    if (tokens.length > 2 && !isSim)
      throw new Error("Usage: connect [auto|usb|serial|ble|sim [model]]");
    return withDeviceLock(async () => {
      if (isSim) {
        if (!DeviceSimulator) throw new Error("The simulator is available in development builds only.");
        const model = tokens[2] || "ep800-crg";
        const sim = new DeviceSimulator({ model, ...activeSimulatorOptions });
        activeSimulator = sim;
        setSimulatorActive(true);
        const passkeyPrompt = async () => "000000";
        const next = new WebBleGatt(sim, passkeyPrompt);
        const info = await adoptPort(next, "sim");
        print(
          `${JSON.stringify(info, null, 2)}\nSelected sim (${model}). Run probe to open the interface.\n`,
        );
        return { code: 0 };
      }
      // Invoke exactly one chooser. A cancellation or failure is returned to the
      // user; it must never cause an unsolicited chooser for another transport.
      const { transport, selected } =
        tokens[1] === "ble"
          ? { transport: "ble", selected: await WebBleGatt.choose() }
          : await requestAdapter(tokens[1] || "auto");
      if (selected?.isSimulator) {
        activeSimulator = selected;
        setSimulatorActive(true);
      } else {
        activeSimulator = null;
        setSimulatorActive(false);
      }
      const next =
        transport === "ble"
          ? new WebBleGatt(selected, requestBlePasskey, clearBlePasskey)
          : transport === "serial"
            ? new WebSerialPort(selected)
            : new WebUsbPort(selected);
      const info = await adoptPort(next, transport);
      print(
        `${JSON.stringify(info, null, 2)}\nSelected ${transport}. Run probe to open the interface.\n`,
      );
      return { code: 0 };
    });
  }
  if (command === "disconnect") {
    return withDeviceLock(async () => {
      cachedBlePasskey = null;
      try {
        sessionStorage.removeItem("esteps_ble_passkey");
      } catch {}
      try {
        if (port) {
          await releaseSelectedPort();
        }
      } finally {
        port = null;
        selectedTransport = null;
        activeSimulator = null;
        setSimulatorActive(false);
      }
      print("Disconnected.\n");
      return { code: 0 };
    });
  }
  if (command === "usb-info" || command === "ble-info" || command === "sim-info") {
    if (!port) throw new Error("Run connect first");
    print(
      `${JSON.stringify({ transport: selectedTransport, ...port.describe() }, null, 2)}\n`,
    );
    return { code: 0 };
  }
  if (command === "import") {
    picker.click();
    return { code: 0 };
  }
  if (command === "files") {
    print(`${JSON.stringify(files.list(), null, 2)}\n`);
    return { code: 0 };
  }
  if (command === "download") {
    if (tokens.length !== 2) throw new Error("Usage: download FILE");
    download(tokens[1], files.bytes(tokens[1]));
    return { code: 0 };
  }
  if (command === "cat") {
    if (tokens.length !== 2) throw new Error("Usage: cat FILE");
    print(`${decoder.decode(files.bytes(tokens[1]))}\n`);
    return { code: 0 };
  }
  if (command === "clear") {
    output.replaceChildren();
    return { code: 0 };
  }
  if (!ready) throw new Error("WASM worker is not ready");
  let toolArgs = tokens;
  if (command === "cranklink" || command === "esteps") toolArgs = tokens.slice(1);
  if (
    !toolArgs.some(
      (argument) => argument === "--port" || argument.startsWith("--port="),
    )
  )
    toolArgs = [
      "--port",
      selectedTransport === "ble" || selectedTransport === "sim"
        ? "webble"
        : selectedTransport === "serial"
          ? "webserial"
          : "webusb",
      ...toolArgs,
    ];
  return withDeviceLock(async () => {
    await files.initialize();
    return new Promise((resolve) => {
      current = { resolve };
      worker.postMessage({ type: "run", args: ["cranklink", ...toolArgs] });
    });
  });
}
form?.addEventListener("submit", (event) => {
  event.preventDefault();
  const line = input.value;
  input.value = "";
  run(line).catch((error) => print(`${error.message}\n`, "stderr"));
});
input?.addEventListener("keydown", (event) => {
  if (event.key === "ArrowUp") {
    event.preventDefault();
    input.value = history[Math.max(0, --historyIndex)] || "";
  }
  if (event.key === "ArrowDown") {
    event.preventDefault();
    historyIndex = Math.min(history.length, historyIndex + 1);
    input.value = history[historyIndex] || "";
  }
});
picker?.addEventListener("change", async () => {
  for (const f of picker.files) {
    try {
      await files.import(f.name, new Uint8Array(await f.arrayBuffer()));
      print(`Imported ${f.name} (${f.size} bytes).\n`);
    } catch (e) {
      print(`${e.message}\n`, "stderr");
    }
  }
  picker.value = "";
});
window.addEventListener("beforeunload", (event) => {
  if (busy || (selectedTransport === "ble" && releaseBleSessionLock)) {
    event.preventDefault();
    event.returnValue = "";
  }
});
navigator.usb?.addEventListener("disconnect", (event) => {
  if (port?.device === event.device) {
    port.disconnected();
    print(
      "USB disconnected; preserve the journal before recovery.\n",
      "stderr",
    );
  }
});
navigator.serial?.addEventListener("disconnect", (event) => {
  if (port?.serialPort === (event.port || event.target)) {
    port.disconnected();
    print(
      "Serial disconnected; preserve the journal before recovery.\n",
      "stderr",
    );
  }
});

window.estepsTerminal = {
  run,
  files,
  output: () => output?.textContent || "",
  status: () => ({
    ready,
    busy,
    usbSelected: !!port,
    transport: selectedTransport,
  }),
  selectGranted,
  ...(import.meta.env.DEV ? {
    get simulator() {
      return activeSimulator;
    },
    configureSimulator: (options) => {
      activeSimulatorOptions = { ...options };
    },
  } : {}),
};
if (import.meta.env.DEV) {
  Object.defineProperty(window, "estepsDeviceSimulator", {
    configurable: true,
    get: () => activeSimulator,
  });
}
try {
  if (!window.crossOriginIsolated)
    throw new Error(
      "Cross-origin isolation is required. Use the bundled localhost server or HTTPS with COOP/COEP headers.",
    );
  shared = {
    control: new SharedArrayBuffer(16),
    buffer: new SharedArrayBuffer(20 * 1024 * 1024),
    simulator: new SharedArrayBuffer(4),
  };
  control = new Int32Array(shared.control);
  buffer = new Uint8Array(shared.buffer);
  await files.initialize();
  startWorker();
  input?.focus();
} catch (e) {
  print(`${e.message}\n`, "stderr");
}
