// Test-only hardware boundary. These are the pinned production browser modules;
// the Web Bluetooth peripheral delegates to stock-firmware simulation in Python.
import { WebBleGatt, characteristicUuid } from '/ble.js';
import { BrowserFiles } from '/files.js';

const enc = new TextEncoder();
const dec = new TextDecoder();
const ids = [0x2af3, 0x2af4, 0x2af9, 0x2afa, 0x2afd, 0x2afe, 0x2aff];
let disconnected = Promise.resolve();
const chars = new Map();
let activeBackup = null;
const backupBeforeWrites = [];

async function peer(request) {
  const reply = await window.firmwarePeer(request);
  if (!reply.ok) throw new Error(reply.error);
  for (const notification of reply.notifications ?? []) {
    const c = chars.get(notification.characteristic);
    c.value = new DataView(Uint8Array.from(notification.value).buffer);
    c.dispatchEvent(new Event('characteristicvaluechanged'));
  }
  return reply.result;
}

class Characteristic extends EventTarget {
  constructor(id) {
    super();
    this.id = id;
    this.uuid = characteristicUuid(id);
    this.properties = {
      read: [0x2af4, 0x2aff].includes(id),
      write: [0x2af3, 0x2afa, 0x2afe, 0x2aff].includes(id),
      writeWithoutResponse: false,
      notify: [0x2af3, 0x2af9, 0x2afd].includes(id),
      indicate: false,
    };
  }
  async startNotifications() {
    await peer({ op: 'request', request: { op: 'subscribe', id: this.id } });
    return this;
  }
  async readValue() {
    const reply = await peer({ op: 'request', request: { op: 'read', id: this.id } });
    return new DataView(Uint8Array.from(reply.bytes).buffer);
  }
  async writeValueWithResponse(value) {
    if (this.id === 0x2afe && ((value[1] === 0x10 && value[2] === 0x10)
      || (value[1] === 0x35 && value[2] === 0))) {
      // Re-open IndexedDB before forwarding the actual SET. An in-memory
      // backup alone cannot demonstrate the client's durable-before-write rule.
      const durable = new BrowserFiles();
      await durable.initialize();
      const backup = JSON.parse(dec.decode(durable.bytes(activeBackup)));
      const parameters = [...value.slice(3)].map(b => b.toString(16).padStart(2, '0')).join('');
      if (!parameters.startsWith(backup.planned_parameters)) throw new Error('Durable backup differs from outgoing SET');
      backupBeforeWrites.push({ setting: backup.setting, before: backup.before, path: activeBackup });
      durable.db.close();
    }
    await peer({ op: 'request', request: { op: 'write', id: this.id, bytes: [...value] } });
  }
}
for (const id of ids) chars.set(id, new Characteristic(id));
const device = new EventTarget();
device.id = 'offline-stock-firmware';
device.name = 'Local firmware simulator';
device.gatt = {
  connected: false,
  async connect() {
    await disconnected;
    await peer({ op: 'connect' });
    this.connected = true;
    return this;
  },
  disconnect() {
    if (!this.connected) return;
    this.connected = false;
    // Web Bluetooth disconnect is synchronous. Serialize its modeled cleanup
    // before any subsequent connect rather than racing Python device ownership.
    disconnected = disconnected.then(() => peer({ op: 'disconnect' }));
    device.dispatchEvent(new Event('gattserverdisconnected'));
  },
  async getPrimaryService() {
    return { getCharacteristics: async () => [...chars.values()] };
  },
};
const port = new WebBleGatt(device, async () => '000000');
const files = new BrowserFiles();
await files.initialize();
const shared = { control: new SharedArrayBuffer(16), buffer: new SharedArrayBuffer(1024 * 1024) };
const control = new Int32Array(shared.control);
const buffer = new Uint8Array(shared.buffer);
const worker = new Worker('/worker.js', { type: 'module' });
let pending;
let output;

async function rpc(message) {
  let response;
  let error = 0;
  try {
    if (message.op.startsWith('ble.')) {
      response = await port.request(message.op, message.payload, message.timeout);
    } else if (message.op.startsWith('fs.')) {
      response = await files.request(message.op, JSON.parse(dec.decode(message.payload)));
    } else {
      throw new Error(`Unexpected browser host operation: ${message.op}`);
    }
  } catch (failure) {
    error = 1;
    response = enc.encode(failure.message);
  }
  if (response.length > buffer.length) throw new Error('Host response exceeds shared buffer');
  if (Atomics.load(control, 3) !== message.id || Atomics.compareExchange(control, 0, 0, 3) !== 0) return;
  buffer.set(response);
  Atomics.store(control, 2, response.length);
  Atomics.store(control, 1, error);
  Atomics.store(control, 0, 1);
  Atomics.notify(control, 0);
}

const ready = new Promise((resolve, reject) => {
  worker.onerror = (event) => {
    const failure = new Error(event.message);
    pending?.reject(failure);
    reject(failure);
  };
  worker.onmessage = ({ data }) => {
    if (data.type === 'ready') resolve();
    else if (data.type === 'rpc') rpc(data).catch(reject);
    else if (data.type === 'output') output?.[data.stream]?.push(data.text);
    else if (data.type === 'done' || data.type === 'ble-disconnected') pending?.resolve(data);
    else if (data.type === 'fatal' || data.type === 'host-failed') {
      const failure = new Error(data.error ?? 'Browser worker host failed');
      pending?.reject(failure);
      reject(failure);
    }
  };
});
worker.postMessage({ type: 'init', shared });

async function call(message) {
  if (pending) throw new Error('Concurrent browser commands are forbidden');
  const promise = new Promise((resolve, reject) => { pending = { resolve, reject }; });
  worker.postMessage(message);
  const timer = setTimeout(() => pending?.reject(new Error('Browser command deadline exceeded')), 60000);
  try {
    return await promise;
  } finally {
    clearTimeout(timer);
    pending = null;
  }
}

async function run(args) {
  output = { stdout: [], stderr: [] };
  activeBackup = args.includes('--backup') ? args[args.indexOf('--backup') + 1] : null;
  const reply = await call({ type: 'run', args: ['cranklink', '--json', '--port', 'webble', '--ble-master', ...args] });
  await files.syncAll();
  await disconnected;
  let value = null;
  const stdout = output.stdout.join('');
  if (reply.code === 0 && stdout.trim()) value = JSON.parse(stdout);
  return { code: reply.code, value, error: output.stderr.join('') };
}

window.browserScenario = async ({ fixed, setting }) => {
  await ready;
  const initial = await peer({ op: 'snapshot' });
  const values = setting === 'lamp' ? [1, 2] : [2150, 2151];
  const commands = [];
  for (const [index, value] of values.entries()) {
    commands.push(await run(['settings', 'set', setting, String(value), '--backup', `save-${index}.json`]));
    if (commands.at(-1).code !== 0) break;
  }
  const read = commands.length === 2 && commands.every(c => c.code === 0)
    ? await run(['settings', 'read', setting]) : null;
  const retained = await peer({ op: 'snapshot' });
  const connectedBeforeDisconnect = device.gatt.connected;
  const durableFiles = new BrowserFiles();
  await durableFiles.initialize();
  const backups = [...durableFiles.files.keys()].filter(path => path.startsWith('save-')).map(path => ({
    path, data: JSON.parse(dec.decode(durableFiles.bytes(path))),
  }));
  durableFiles.db.close();
  const disconnect = fixed ? await call({ type: 'ble-disconnect' }) : { code: 0, alreadyClosedByOldClient: true };
  await port.close();
  await disconnected;
  const stopped = await peer({ op: 'snapshot' });
  const powerCycle = await peer({ op: 'power_cycle' });
  await peer({ op: 'reenable' });
  const afterPowerCycle = await run(['settings', 'read', setting]);
  // Fixed code retains even this diagnostic read; release it before shutdown.
  if (fixed && afterPowerCycle.code === 0) await call({ type: 'ble-disconnect' });
  await port.close();
  await disconnected;
  const final = await peer({ op: 'snapshot' });
  worker.terminate();
  return { initial, commands, read, retained, connectedBeforeDisconnect, backups, backupBeforeWrites,
    disconnect, stopped, powerCycle, afterPowerCycle, final };
};
window.harnessReady = true;
