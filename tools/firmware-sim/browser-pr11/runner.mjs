// Builds stay in Python's temporary directory; only sanitized evidence is saved.
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { readFile, writeFile, mkdtemp } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { createInterface } from 'node:readline';
import { dirname, extname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../../..');
const options = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, index, args) => {
  if (index % 2 === 0) pairs.push([value.slice(2), args[index + 1]]);
  return pairs;
}, []));
const require = createRequire(import.meta.url);
const { chromium } = require(options['playwright-module'] ?? require.resolve('@playwright/test', { paths: [join(root, 'web-instrument')] }));
const browser = await chromium.launch();
const results = [];
const runDirectory = await mkdtemp(join(options['work-dir'], 'run-'));
const BASE = '0cfb180f576544d75a63a7fbe36f38670ae8679c';
const FIXED = '158e8f5bc8d5e379c08ee3fa4ca4d59c9af10789';

function pceCount(snapshot, command, params) {
  return snapshot.events.filter(event => event.kind === 'pce-command'
    && event.command === command && event.params === params).length;
}

function verify(result) {
  const fixed = result.revision === 'after';
  const seed = options.setting === 'lamp' ? 0 : 2149;
  const values = options.setting === 'lamp' ? [1, 2] : [2150, 2151];
  const field = options.setting === 'lamp' ? 'lamp' : 'tire_circumference';
  const blocked = !fixed && result.radioLifecycle;
  assert.equal(result.initial.storage[field], seed, 'Every case uses fresh synthetic storage');
  assert.equal(result.commands.length, 2, 'Both saves must be attempted');
  for (let index = 0; index < (blocked ? 1 : 2); index++) {
    const command = result.commands[index];
    assert.equal(command.code, 0, `Save ${index + 1} succeeded`);
    assert.equal(command.error, '');
    assert.equal(command.value.setting, options.setting);
    assert.equal(command.value.before, index === 0 ? seed : values[0]);
    assert.equal(command.value.after, values[index]);
    assert.equal(command.value.changed, true, 'The save must mutate a value, not skip a no-op');
  }
  if (blocked) {
    assert.equal(result.commands[1].code, 1);
    assert.match(result.commands[1].error, /Gateway connection mode ended/);
    assert.equal(result.read, null, 'Do not retry the failed transaction');
    assert.equal(result.retained.radio_available, false);
  } else {
    assert.equal(result.read.code, 0);
    assert.equal(result.read.value.value, values[1]);
  }
  const successfulSaves = blocked ? 1 : 2;
  const expectedValue = values[successfulSaves - 1];
  assert.equal(result.retained.storage[field], expectedValue);
  assert(result.retained.storage.commits >= successfulSaves, 'Firmware commit boundary was reached');
  assert.equal(result.backups.length, successfulSaves);
  assert.equal(result.backupBeforeWrites.length, successfulSaves, 'Every actual SET has a durable backup first');
  for (const [index, backup] of result.backups.entries()) {
    assert.equal(backup.data.format, 'cranklink-setting-backup-v1');
    assert.equal(backup.data.serial, '000000000000');
    assert.equal(backup.data.slot, 13);
    assert.equal(backup.data.setting, options.setting);
    assert.equal(backup.data.before, index === 0 ? seed : values[0]);
  }
  if (fixed) {
    assert.equal(result.retained.connections, 1);
    assert.equal(result.retained.connection_active, true);
    assert.equal(result.connectedBeforeDisconnect, true);
    assert.equal(result.retained.motor_connected, false, 'Pause releases the selected motor');
    assert.equal(pceCount(result.retained, 0x0c, '01'), 1, 'Only one initial gateway start');
    assert.equal(pceCount(result.retained, 0x0c, '00'), 0, 'No gateway stop between commands');
    assert.equal(pceCount(result.retained, 0x1d, '00'), 1, 'PnP discovery is retained');
    assert.equal(result.retained.lifecycle_counts['gatt-disconnect'] ?? 0, 0);
    assert.equal(result.disconnect.type, 'ble-disconnected');
    assert.equal(result.disconnect.code, 0);
    assert.equal(pceCount(result.stopped, 0x0c, '00'), 1, 'Explicit disconnect sends the final gateway stop');
  } else {
    assert.equal(result.connectedBeforeDisconnect, false);
    assert.equal(result.retained.connection_active, false);
    assert.equal(result.retained.connections, blocked ? 1 : 3);
    assert.equal(pceCount(result.retained, 0x0c, '00'), blocked ? 1 : 3);
  }
  assert.equal(result.stopped.connection_active, false);
  assert.equal(result.stopped.motor_connected, false);
  assert.equal(result.powerCycle.storage.commits, 0, 'Power cycle reconstructs a new firmware machine');
  assert.equal(result.powerCycle.storage[field], expectedValue, 'Storage survives new motor construction');
  assert.equal(result.afterPowerCycle.code, 0);
  assert.equal(result.afterPowerCycle.value.value, expectedValue, 'Actual firmware GET confirms disk reload');
  assert.equal(result.final.connection_active, false);
  assert.equal(result.final.motor_connected, false);
  return {
    source_revision: fixed ? FIXED : BASE,
    modeled_connection_mode_ends_on_gateway_stop: result.radioLifecycle,
    successful_saves: successfulSaves,
    second_save_succeeded: !blocked,
    retained_connection_contract_passed: fixed,
    connections_before_disconnect: result.retained.connections,
    gateway_stops_before_disconnect: pceCount(result.retained, 0x0c, '00'),
    initial_value: seed,
    saved_values: result.commands.filter(command => command.code === 0).map(command => command.value.after),
    firmware_read_after_power_cycle: result.afterPowerCycle.value.value,
    durable_backups_verified_before_set: result.backupBeforeWrites.length,
    firmware_commits_before_power_cycle: result.retained.storage.commits,
    failure: blocked ? 'Gateway connection mode ended on reconnect before second save' : null,
    cleanup_confirmed: true,
  };
}

function startPeer(state, radioLifecycle) {
  const command = [join(root, 'tools/firmware-sim/browser_peer.py'), '--gateway', options.gateway,
    '--motor', options.motor, '--state', state];
  if (radioLifecycle) command.push('--radio-lifecycle');
  const process = spawn(options.python, command, { stdio: ['pipe', 'pipe', 'pipe'] });
  let sequence = 0;
  let diagnostics = '';
  const pending = new Map();
  process.stderr.on('data', data => { diagnostics += data; });
  createInterface({ input: process.stdout }).on('line', line => {
    try {
      const message = JSON.parse(line);
      const callback = pending.get(message.id);
      assert(callback, 'Unsolicited firmware peer response');
      pending.delete(message.id);
      callback.resolve(message);
    } catch (error) {
      for (const callback of pending.values()) callback.reject(error);
      pending.clear();
    }
  });
  process.on('exit', code => {
    for (const callback of pending.values()) callback.reject(new Error(`Firmware peer exited ${code}: ${diagnostics}`));
    pending.clear();
  });
  let chain = Promise.resolve();
  return {
    request(message) {
      const operation = chain.then(() => new Promise((resolve, reject) => {
        const id = ++sequence;
        pending.set(id, { resolve, reject });
        process.stdin.write(`${JSON.stringify({ ...message, id })}\n`);
      }));
      chain = operation.catch(() => {});
      return operation;
    },
    async close() {
      process.stdin.end();
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => { process.kill(); reject(new Error('Firmware peer shutdown timeout')); }, 5000);
        process.once('exit', code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error(diagnostics)); });
      });
      assert.equal(diagnostics, '', 'Unexpected firmware simulator diagnostics');
    },
  };
}

try {
  for (const name of ['before', 'after']) {
    const site = resolve(options['work-dir'], name, 'web-instrument');
    const server = createServer(async (request, response) => {
      response.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
      response.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');
      response.setHeader('Cache-Control', 'no-store');
      try {
        const pathname = new URL(request.url, 'http://localhost').pathname;
        if (pathname === '/') {
          response.setHeader('Content-Type', 'text/html');
          response.end('<!doctype html><meta charset="utf-8"><title>Firmware browser regression</title><script type="module" src="/harness.js"></script>');
          return;
        }
        const path = pathname === '/harness.js' ? join(here, 'page.js') : resolve(site, `.${pathname}`);
        assert(path === join(here, 'page.js') || path.startsWith(site + sep), 'Invalid static file path');
        response.setHeader('Content-Type', extname(path) === '.wasm' ? 'application/wasm' : 'application/javascript');
        response.end(await readFile(path));
      } catch (error) {
        response.statusCode = 404;
        response.end('Not found');
      }
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    try {
      for (const radioLifecycle of [false, true]) {
        const label = `${name}-${radioLifecycle ? 'connection-mode' : 'always-available'}`;
        const peer = startPeer(join(runDirectory, `${label}-${options.setting}.local.json`), radioLifecycle);
        const context = await browser.newContext();
        const page = await context.newPage();
        const pageErrors = [];
        page.on('pageerror', error => pageErrors.push(error.message));
        try {
          await page.exposeFunction('firmwarePeer', message => peer.request(message));
          await page.goto(`http://127.0.0.1:${server.address().port}`);
          await page.waitForFunction(() => window.harnessReady, { timeout: 30000 });
          const result = await page.evaluate(args => window.browserScenario(args), { fixed: name === 'after', setting: options.setting });
          assert.deepEqual(pageErrors, [], 'No browser runtime errors');
          const evidence = verify({ revision: name, radioLifecycle, ...result });
          evidence.wasm_sha256 = createHash('sha256').update(await readFile(join(site, 'pkg/cranklink_bg.wasm'))).digest('hex');
          results.push(evidence);
          console.log(`${label}: ${result.commands.map(c => c.code).join(',')} saved; connected=${result.connectedBeforeDisconnect}`);
        } finally {
          await context.close();
          await peer.close();
        }
      }
    } finally {
      await new Promise(resolve => server.close(resolve));
    }
  }
  // No raw transport, authentication, vendor images or browser backups are saved.
  await writeFile(options.output, JSON.stringify({
    schema_version: 1, passed: true, setting: options.setting,
    level: 'unchanged pinned production WASM worker, WebBleGatt and BrowserFiles in Chromium with stock gateway/motor functions',
    gateway: 'sc-em800-4.6.0', motor: 'ep800-4.2.0',
    modeled: ['radio/pairing', 'EEPROM commit and boot cache hydration', 'initialized RAM/MMIO',
      'PnP delivery and logical address handoff', 'motor M version peer'],
    not_validated: ['physical RF', 'complete UI click workflow', 'full hardware boot', 'power loss during a commit'],
    results,
  }, null, 2) + '\n');
} finally {
  await browser.close();
}
