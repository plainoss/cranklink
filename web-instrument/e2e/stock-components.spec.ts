import { test, expect, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { mkdtemp, readFile, rm, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHash } from "node:crypto";

// Opt in with a local manifest containing sc-em800-4.6.0 and mu-ur500-3.3.1.
// No firmware is downloaded or bundled. The ordinary E2Es use scripted peers.
const manifestPath = process.env.CRANKLINK_COMPONENT_BROWSER_FIXTURES;
test.skip(!manifestPath, "Exact stock component firmware requires a local fixture manifest");

async function startPeer(kind: string, fixture: string) {
  const manifest = JSON.parse(await readFile(manifestPath!, "utf8"));
  if (!manifest[fixture] || !manifest["sc-em800-4.6.0"]) throw new Error(`Missing local firmware fixture: ${fixture}`);
  const directory = await mkdtemp(join(tmpdir(), "cranklink-component-browser-"));
  const child = spawn(process.env.CRANKLINK_SIM_PYTHON ?? "python3", [
    resolve("../tools/firmware-sim/component_browser_peer.py"),
    "--gateway", manifest["sc-em800-4.6.0"], "--component", manifest[fixture],
    "--kind", kind, "--state", join(directory, "storage.json"),
  ], { stdio: ["pipe", "pipe", "pipe"] });
  let diagnostics = "";
  child.stderr.on("data", (data) => { diagnostics += data; });
  let sequence = 0;
  const pending = new Map<number, { resolve: (result: any) => void; reject: (error: Error) => void }>();
  createInterface({ input: child.stdout }).on("line", (line) => {
    try {
      const reply = JSON.parse(line);
      const waiting = pending.get(reply.id);
      if (!waiting) throw new Error("Unexpected component firmware peer response");
      pending.delete(reply.id);
      waiting.resolve(reply);
    } catch (error) {
      for (const waiting of pending.values()) waiting.reject(error as Error);
      pending.clear();
    }
  });
  child.on("exit", (code) => {
    for (const waiting of pending.values()) waiting.reject(new Error(`Component firmware peer exited ${code}: ${diagnostics}`));
    pending.clear();
  });
  let chain = Promise.resolve();
  return {
    request(message: any): Promise<any> {
      const operation = chain.then(() => new Promise((resolve, reject) => {
        if (child.exitCode !== null) return reject(new Error(`Component firmware peer exited: ${diagnostics}`));
        const id = ++sequence;
        pending.set(id, { resolve, reject });
        child.stdin.write(`${JSON.stringify({ ...message, id })}\n`);
      }));
      chain = operation.then(() => {}, () => {});
      return operation;
    },
    async close() {
      child.stdin.end();
      await new Promise<void>((done) => {
        if (child.exitCode !== null) return done();
        const timer = setTimeout(() => { child.kill("SIGKILL"); done(); }, 5000);
        child.once("exit", () => { clearTimeout(timer); done(); });
      });
      await rm(directory, { recursive: true, force: true });
    },
  };
}

async function installFirmwareDevice(page: Page) {
  await page.evaluate(async () => {
    // This is only a peripheral UUID declaration. WebBleGatt itself is the
    // unchanged production module bundled in the application under test.
    const characteristicUuid = (id: number) => `0000${id.toString(16).padStart(4, "0")}-5348-494d-414e-4f5f424c4500`;
    const chars = new Map<number, any>();
    const audits: any[] = [];
    const seen = new Set<string>();
    (window as any).stockBackupAudits = audits;
    let disconnected = Promise.resolve();
    async function peer(message: any) {
      const reply = await (window as any).componentFirmwarePeer(message);
      if (!reply.ok) throw new Error(reply.error);
      for (const notification of reply.notifications ?? []) {
        const characteristic = chars.get(notification.characteristic);
        characteristic.value = new DataView(Uint8Array.from(notification.value).buffer);
        characteristic.dispatchEvent(new Event("characteristicvaluechanged"));
      }
      return reply.result;
    }
    class Characteristic extends EventTarget {
      id: number;
      uuid: string;
      value: DataView | null = null;
      properties = { read: true, write: true, writeWithoutResponse: false, notify: true, indicate: false };
      constructor(id: number) { super(); this.id = id; this.uuid = characteristicUuid(id); }
      async startNotifications() { await peer({ op: "request", request: { op: "subscribe", id: this.id } }); return this; }
      async readValue() {
        const result = await peer({ op: "request", request: { op: "read", id: this.id } });
        return new DataView(Uint8Array.from(result.bytes).buffer);
      }
      async writeValueWithResponse(value: Uint8Array) {
        const [slot, group, command, ...parameters] = [...value];
        const isSet = new Set(["11:16", "11:24", "35:0", "5:24", "5:32", "5:80"]).has(`${group}:${command}`);
        if (this.id === 0x2afe && isSet) {
          const database = await new Promise<IDBDatabase>((resolve, reject) => {
            const request = indexedDB.open("esteps-files-v1", 1);
            request.onsuccess = () => resolve(request.result);
            request.onerror = () => reject(request.error);
          });
          try {
            const records = await new Promise<any[]>((resolve, reject) => {
              const transaction = database.transaction("files", "readonly");
              const request = transaction.objectStore("files").getAll();
              transaction.oncomplete = () => resolve(request.result);
              transaction.onerror = () => reject(transaction.error);
              transaction.onabort = () => reject(transaction.error);
            });
            const planned = parameters.map((b) => b.toString(16).padStart(2, "0")).join("");
            const identity = (window as any).cranklinkInstrumentController.getState().identity;
            const backups = records.flatMap(({ path, bytes }: { path: string; bytes: ArrayBuffer }) => {
              if (seen.has(path) || !path.endsWith(".json")) return [];
              try {
                const data = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes)));
                return data.format === "cranklink-setting-backup-v1" && data.planned_parameters === planned &&
                  data.serial.toUpperCase() === identity.serial && data.settings_model === identity.model ? [{ path, data }] : [];
              } catch { return []; }
            });
            if (backups.length !== 1) throw new Error(`Expected fresh durable component backup before stock SET; found ${backups.length}`);
            seen.add(backups[0].path);
            audits.push({ slot, group, command, parameters, backup: backups[0] });
          } finally { database.close(); }
        }
        await peer({ op: "request", request: { op: "write", id: this.id, bytes: [...value] } });
      }
    }
    for (const id of [0x2af3, 0x2af4, 0x2af9, 0x2afa, 0x2afd, 0x2afe, 0x2aff]) chars.set(id, new Characteristic(id));
    const device: any = new EventTarget();
    device.id = "local-stock-component";
    device.name = "Local stock firmware peer";
    // Uses the production logical-time fast path; no command or reply bypass.
    device.isSimulator = true;
    device.gatt = {
      connected: false,
      async connect() { await disconnected; await peer({ op: "connect" }); this.connected = true; return this; },
      disconnect() {
        if (!this.connected) return;
        this.connected = false;
        disconnected = disconnected.then(() => peer({ op: "disconnect" }));
        device.dispatchEvent(new Event("gattserverdisconnected"));
      },
      async getPrimaryService() { return { getCharacteristics: async () => [...chars.values()] }; },
    };
    Object.defineProperty(navigator, "bluetooth", { configurable: true, value: { requestDevice: async () => device } });
    (window as any).awaitStockDisconnect = () => disconnected;
  });
}

async function connectWithPasskey(page: Page) {
  await page.getByTestId("btn-connect-ble").click();
  const dialog = page.getByRole("dialog", { name: "Bluetooth passkey", exact: true });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel("Passkey", { exact: true }).fill("000000");
  await dialog.getByRole("button", { name: "Authenticate", exact: true }).click();
}

for (const fixture of [
  { kind: "display", profile: "sc-em800-4.6.0", model: "SC-EM800", settings: [["display-language", 2], ["display-brightness", 100]] as const, before: { "display-language": 0, "display-brightness": 60 } },
  { kind: "mu", profile: "mu-ur500-3.3.1", model: "MU-UR500", settings: [["hub-gear-count", 8]] as const, before: { "hub-gear-count": 11 } },
]) {
  test(`stock ${fixture.model}: UI batch, durable backup, controlled power cycle and rollback`, async ({ page }, testInfo) => {
    test.setTimeout(120_000);
    page.setDefaultTimeout(15_000);
    const peer = await startPeer(fixture.kind, fixture.profile);
    try {
      await page.exposeFunction("componentFirmwarePeer", (message: any) => peer.request(message));
      await page.goto("./");
      await expect.poll(() => page.evaluate(() => Boolean((window as any).estepsTerminal?.status().ready)), { timeout: 30_000 }).toBe(true);
      const warning = page.getByTestId("btn-dismiss-warning");
      if (await warning.isVisible()) await warning.click();
      await installFirmwareDevice(page);
      await connectWithPasskey(page);
      await expect(page.getByTestId("identity-strip")).toContainText(fixture.model);
      const initial = (await peer.request({ op: "snapshot" })).result;
      for (const [name, value] of fixture.settings) {
        const choices: number[] = await page.evaluate((setting) => (window as any).cranklinkInstrumentController.getState().schema.find((s: any) => s.name === setting).choices, name);
        await page.getByTestId(`setting-select-${name}`).getByRole("button").click();
        await page.getByRole("option").nth(choices.indexOf(value)).click();
      }
      await page.getByTestId("btn-review-save").click();
      await page.getByTestId("btn-commit-batch").click();
      await expect(page.getByTestId("transaction-rail")).toBeHidden();
      const saved = (await peer.request({ op: "snapshot" })).result;
      expect(saved.component_storage.commits).toBe(fixture.settings.length);
      const audits = await page.evaluate(() => (window as any).stockBackupAudits);
      expect(audits).toHaveLength(fixture.settings.length);
      await page.evaluate(async () => {
        await (window as any).cranklinkInstrumentController.disconnect();
        await (window as any).awaitStockDisconnect();
      });
      const cycled = (await peer.request({ op: "power_cycle" })).result;
      expect(cycled.component_storage.commits).toBe(0);
      await connectWithPasskey(page);
      await expect(page.getByTestId("identity-strip")).toContainText(fixture.model);
      const persisted = await page.evaluate(() => Object.fromEntries(Object.entries((window as any).cranklinkInstrumentController.getState().liveValues).map(([name, entry]) => [name, (entry as any).value])));
      for (const [name, value] of fixture.settings) expect(persisted[name]).toBe(value);
      await page.getByTestId("nav-section-journal").click();
      await page.locator('[data-action="restore-snapshot"]').first().click();
      await page.getByTestId("btn-confirm-restore").click();
      await expect(page.getByRole("region").getByText("Restored the backup", { exact: false })).toBeVisible();
      const restored = await page.evaluate(() => Object.fromEntries(Object.entries((window as any).cranklinkInstrumentController.getState().liveValues).map(([name, entry]) => [name, (entry as any).value])));
      expect(restored).toMatchObject(fixture.before);
      let final = (await peer.request({ op: "snapshot" })).result;
      expect(final.component_storage.commits).toBe(fixture.settings.length);
      expect(await page.evaluate(() => (window as any).stockBackupAudits.length)).toBe(fixture.settings.length * 2);
      let actuatorOutcome: any = null;
      if (fixture.kind === "mu") {
        // The exact MU fixture executes acceptance but supplies no physical
        // actuator completion. The UI must retain the target and uncertainty,
        // rather than reporting a saved adjustment or replaying the SET.
        await page.evaluate(() => { window.location.hash = "#/tune/all"; });
        const adjustment = page.getByTestId("setting-input-hub-adjustment").locator("input");
        await adjustment.fill("-1");
        await adjustment.press("Tab");
        await page.getByTestId("btn-review-save").click();
        await page.getByTestId("btn-commit-batch").click();
        await expect(page.getByTestId("error-banner")).toContainText(/completion unconfirmed|outcome may be uncertain/i);
        expect(await page.evaluate(() => (window as any).cranklinkInstrumentController.getState().failure.error))
          .toMatch(/hub adjustment readback mismatch: actuator completion unconfirmed/i);
        await expect(page.getByTestId("transaction-rail")).toBeVisible();
        const state = await page.evaluate(() => {
          const s = (window as any).cranklinkInstrumentController.getState();
          return { live: s.liveValues["hub-adjustment"].value, staged: s.stagedValues["hub-adjustment"], failure: s.failure?.kind, phase: s.phase };
        });
        expect(state).toEqual({ live: 0, staged: -1, failure: "save", phase: "ready" });
        final = (await peer.request({ op: "snapshot" })).result;
        const adjustmentSets = final.events.filter((e: any) => e.kind === "component-free-command" && e.group === 11 && e.command === 0x10).length;
        expect(adjustmentSets).toBe(1);
        const adjustmentSetIndex = final.events.findIndex((e: any) => e.kind === "component-free-command" && e.group === 11 && e.command === 0x10);
        const adjustmentPolls = final.events.slice(adjustmentSetIndex + 1).filter((e: any) => e.kind === "component-free-command" && e.group === 11 && e.command === 0x14).length;
        expect(adjustmentPolls).toBeGreaterThan(1);
        expect(final.component_storage.commits).toBe(1);
        expect(await page.evaluate(() => (window as any).stockBackupAudits.length)).toBe(3);
        actuatorOutcome = { accepted_sets: adjustmentSets, automatic_retries: adjustmentSets - 1, firmware_readback_polls_after_ack: adjustmentPolls, ...state };
      }
      await page.evaluate(async () => {
        await (window as any).cranklinkInstrumentController.disconnect();
        await (window as any).awaitStockDisconnect();
      });
      const stopped = (await peer.request({ op: "snapshot" })).result;
      expect(stopped.connection_active).toBe(false);
      expect(stopped.motor_connected).toBe(false);
      const wasm = await (await page.request.get("/pkg/cranklink_bg.wasm")).body();
      const moduleEntry = await page.locator("script[type='module'][src]").evaluateAll((scripts) =>
        scripts.map((script) => (script as HTMLScriptElement).src).find((src) => /\/assets\/index-.*\.js$/.test(src)) ?? null);
      const moduleEntryHash = moduleEntry ? createHash("sha256").update(await (await page.request.get(moduleEntry)).body()).digest("hex") : null;
      const report = { profile: fixture.profile, gateway_sha256: initial.gateway_sha256, component_sha256: initial.component_sha256,
        browser_wasm_sha256: createHash("sha256").update(wasm).digest("hex"),
        browser_module_entry: moduleEntry ? new URL(moduleEntry).pathname : null, browser_module_entry_sha256: moduleEntryHash,
        saved: Object.fromEntries(fixture.settings), persisted, restored,
        stock_commits_before_cycle: saved.component_storage.commits, stock_commits_for_restore: final.component_storage.commits,
        durable_backups_before_actual_set: await page.evaluate(() => (window as any).stockBackupAudits.length),
        actuator_completion_unconfirmed: actuatorOutcome, final_component_instructions: final.component_instructions,
        cleanup_confirmed: !stopped.connection_active && !stopped.motor_connected,
        modeled: ["GATT radio/PIN/notification delivery", "initialized RAM and boot-ready caches", "physical NVM commit and controlled boot hydration", "logical timing"],
        unvalidated: ["physical hardware", "MU actuator completion", "power loss during commit", "full firmware reset/boot"] };
      const json = JSON.stringify(report, null, 2) + "\n";
      await testInfo.attach("stock-component-browser-evidence", { body: json, contentType: "application/json" });
      if (process.env.CRANKLINK_COMPONENT_BROWSER_REPORT_DIR) {
        await mkdir(process.env.CRANKLINK_COMPONENT_BROWSER_REPORT_DIR, { recursive: true });
        await writeFile(join(process.env.CRANKLINK_COMPONENT_BROWSER_REPORT_DIR, `${fixture.profile}.json`), json);
      }
    } finally { await peer.close(); }
  });
}
