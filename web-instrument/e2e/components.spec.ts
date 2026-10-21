import { test, expect, type Page } from "@playwright/test";

// These are scripted wire peers. Exact stock display/MU firmware execution is
// covered separately by tools/firmware-sim; a browser fixture is not that proof.
async function connectComponents(page: Page) {
  await page.goto("./");
  await expect.poll(() => page.evaluate(() => window.crossOriginIsolated &&
    Boolean((window as any).estepsTerminal?.status().ready)), { timeout: 30_000 }).toBe(true);
  const warning = page.getByTestId("btn-dismiss-warning");
  if (await warning.isVisible()) await warning.click();
  await page.getByText("Include Di2 components", { exact: true }).click();
  await expect(page.getByRole("checkbox", { name: "Include Di2 components" })).toBeChecked();
  await page.getByTestId("btn-connect-sim").click();
  await expect(page.getByTestId("identity-strip")).toContainText("EP800-CRG");
  expect(await page.evaluate(() => ({
    configured: (window as any).cranklinkInstrumentController.getState().simulatorOptions.components,
    peers: (window as any).estepsDeviceSimulator.components.size,
  }))).toEqual({ configured: true, peers: 6 });
  await expect.poll(() => page.evaluate(() =>
    (window as any).cranklinkInstrumentController.getState().units.length)).toBe(7);
  await installDurableBackupAudit(page);
}

async function selectComponent(page: Page, slot: number, model: string) {
  await page.getByTestId("component-select").getByRole("button").click();
  await page.getByRole("option", { name: `${model} · slot ${slot}`, exact: true }).click();
  await expect(page.getByTestId("identity-strip")).toContainText(model);
  await expect.poll(() => page.evaluate(() =>
    (window as any).cranklinkInstrumentController.getState().phase)).toBe("ready");
  await expect(page.getByTestId("all-settings")).toBeVisible();
}

async function stageChoice(page: Page, name: string, choice: number) {
  const choices: number[] = await page.evaluate((setting) =>
    (window as any).cranklinkInstrumentController.getState().schema.find((s: any) => s.name === setting).choices, name);
  expect(choices).toContain(choice);
  await page.getByTestId(`setting-select-${name}`).getByRole("button").click();
  await page.getByRole("option").nth(choices.indexOf(choice)).click();
  await expect(page.getByTestId(`setting-item-${name}`)).toContainText("Staged:");
}

async function save(page: Page, count: number) {
  await expect(page.getByTestId("transaction-rail")).toContainText(`${count} change${count === 1 ? "" : "s"} to save`);
  await page.getByTestId("btn-review-save").click();
  await expect(page.getByTestId("review-dialog")).toBeVisible();
  await page.getByTestId("btn-commit-batch").click();
  await expect(page.getByTestId("transaction-rail")).toBeHidden();
  await expect.poll(() => page.evaluate(() =>
    (window as any).cranklinkInstrumentController.getState().phase)).toBe("ready");
}

async function values(page: Page) {
  return page.evaluate(() => Object.fromEntries(Object.entries(
    (window as any).cranklinkInstrumentController.getState().liveValues,
  ).map(([key, entry]) => [key, (entry as any).value])));
}

async function registers(page: Page, slot: number) {
  return page.evaluate((target) => (window as any).estepsDeviceSimulator.getComponentRegisters(target), slot);
}

// Inspect a fresh IndexedDB connection at the actual GATT write boundary. The
// Rust filesystem bridge must have committed a new backup before each SET.
async function installDurableBackupAudit(page: Page) {
  await page.evaluate(() => {
    const terminal = (window as any).estepsTerminal;
    const simulator = (window as any).estepsDeviceSimulator;
    const characteristic = simulator.chars.find((c: any) => c.id === 0x2afe);
    const setCommands = new Set(["4:16", "11:80", "10:152", "11:96", "11:16", "11:24", "35:0", "5:24", "5:32", "5:80"]);
    const seen = new Set<string>();
    const audits: any[] = [];
    (window as any).componentBackupAudits = audits;
    const write = characteristic.writeValueWithResponse.bind(characteristic);
    characteristic.writeValueWithResponse = async (bytes: Uint8Array) => {
      const [slot, group, command, ...parameters] = [...bytes];
      if (simulator.components.has(slot) && setCommands.has(`${group}:${command}`)) {
        const modulePath = "/files.js";
        const { BrowserFiles } = await import(modulePath);
        const disk = new BrowserFiles();
        await disk.initialize();
        try {
          const planned = parameters.map((b) => b.toString(16).padStart(2, "0")).join("");
          const component = simulator.components.get(slot);
          const serial = component.serial.map((b: number) => b.toString(16).padStart(2, "0")).join("");
          const backups = disk.list().flatMap(({ path }: { path: string }) => {
            if (!path.endsWith(".json") || seen.has(path)) return [];
            try {
              const data = JSON.parse(new TextDecoder().decode(disk.bytes(path)));
              return data.format === "cranklink-setting-backup-v1" && data.serial === serial &&
                data.planned_parameters === planned &&
                (data.settings_model === undefined || data.settings_model === component.model) ? [{ path, data }] : [];
            } catch { return []; }
          });
          if (backups.length !== 1) throw new Error(`Expected one new durable backup before ${slot}/${group}/${command}; found ${backups.length}`);
          seen.add(backups[0].path);
          audits.push({ slot, group, command, parameters, backup: backups[0] });
        } finally { disk.db.close(); }
      }
      return write(bytes);
    };
  });
}

test("component inventory, packed lever save and durable rollback", async ({ page }) => {
  await connectComponents(page);
  const units = await page.evaluate(() => (window as any).cranklinkInstrumentController.getState().units);
  expect(units.map((u: any) => [u.slot, u.model, u.model_series, u.model_unit])).toEqual([
    [4, "ST-6870-L", 7, 1], [5, "RD-R9150", 17, 4], [6, "FD-6870", 7, 3],
    [7, "BT-DN110", 17, 0], [8, "MU-UR500", 32, 4], [9, "SC-EM800", 35, 2],
    [13, "DU-EP800-CRG", 35, 0],
  ]);
  await selectComponent(page, 4, "ST-6870-L");
  await expect(page.getByTestId("identity-strip")).toContainText("C00400000001");
  expect(await values(page)).toMatchObject({ "switch-a-function": 2, "switch-b-function": 3, "switch-c-function": 10, "switch-s-function": 15 });
  await page.getByTestId("nav-section-journal").click();
  await page.getByTestId("btn-capture-snapshot").click();
  await expect(page.getByTestId("restore-point")).toHaveCount(1);
  await expect(page.getByTestId("restore-point")).toContainText(/full backup|full configuration|download/i);
  await expect(page.locator('[data-action="restore-snapshot"]')).toHaveCount(0);
  await page.getByTestId("nav-group-all").click();
  await expect(page.getByTestId("readonly-switch-c-function")).toHaveText("Read only");
  await expect(page.getByTestId("setting-select-switch-c-function")).toHaveCount(0);
  await stageChoice(page, "switch-a-function", 1);
  await stageChoice(page, "switch-b-function", 0);
  await save(page, 2);
  expect(await registers(page, 4)).toEqual({ switchAB: 0x10, switchCS: 0xaf });
  expect(await values(page)).toMatchObject({ "switch-a-function": 1, "switch-b-function": 0 });
  const audit = await page.evaluate(() => (window as any).componentBackupAudits);
  expect(audit.map((a: any) => a.parameters)).toEqual([[0x13, 0xaf, 0, 0], [0x10, 0xaf, 0, 0]]);
  expect(audit.map((a: any) => a.backup.data.before)).toEqual([2, 3]);
  await page.getByTestId("nav-section-journal").click();
  await page.locator('[data-action="restore-snapshot"]').first().click();
  await page.getByTestId("btn-confirm-restore").click();
  await expect(page.getByRole("region").getByText("Restored the backup", { exact: false })).toBeVisible();
  expect(await registers(page, 4)).toEqual({ switchAB: 0x23, switchCS: 0xaf });
  expect(await page.evaluate(() => (window as any).componentBackupAudits.length)).toBe(4);
});

test("derailleur, power, hub and display settings use their own wire layouts", async ({ page }) => {
  await connectComponents(page);
  await selectComponent(page, 5, "RD-R9150");
  expect(await values(page)).toMatchObject({ "derailleur-adjustment": -1, "derailleur-adjustment-maximum": 16, "multi-shift-speed": 3 });
  await expect(page.getByTestId("readonly-derailleur-adjustment")).toHaveText("Read only");
  const protection = await page.evaluate(() => {
    try { (window as any).cranklinkInstrumentController.stageParameter("derailleur-adjustment", 1); return "accepted"; }
    catch (error) { return String(error); }
  });
  expect(protection).toMatch(/read.only/i);
  await stageChoice(page, "multi-shift-speed", 5);
  await save(page, 1);
  expect(await registers(page, 5)).toMatchObject({ adjustment: -1, adjustmentMaximum: 16, multiShiftSpeed: 5 });

  await selectComponent(page, 6, "FD-6870");
  await stageChoice(page, "synchro-shift-interval", 2);
  await save(page, 1);
  expect(await registers(page, 6)).toMatchObject({ adjustment: 0, synchroInterval: 2 });

  await selectComponent(page, 7, "BT-DN110");
  await stageChoice(page, "multi-shift-limit.up", 2);
  await save(page, 1);
  expect(await registers(page, 7)).toEqual({ shiftLimitUp: 2, shiftLimitDown: 3 });

  await selectComponent(page, 8, "MU-UR500");
  await stageChoice(page, "hub-gear-count", 8);
  await save(page, 1);
  expect(await registers(page, 8)).toMatchObject({ adjustment: 0, minimumGear: 1, gearCount: 8 });
  const adjustment = page.getByTestId("setting-input-hub-adjustment").locator("input");
  await adjustment.fill("-3");
  await adjustment.press("Tab");
  await expect(page.getByTestId("setting-item-hub-adjustment")).toContainText("Staged:");
  await save(page, 1);
  expect(await registers(page, 8)).toMatchObject({ adjustment: -3, minimumGear: 1, gearCount: 8 });

  await selectComponent(page, 9, "SC-EM800");
  await stageChoice(page, "display-language", 2);
  await stageChoice(page, "display-brightness", 100);
  await save(page, 2);
  expect(await registers(page, 9)).toEqual({ beep: 1, distanceUnit: 0, language: 2, brightness: 100 });
  const audit = await page.evaluate(() => (window as any).componentBackupAudits);
  expect(audit.map((a: any) => [a.slot, a.group, a.command, a.parameters])).toEqual([
    [5, 11, 0x50, [5, 0, 0, 0]], [6, 10, 0x98, [2]], [7, 11, 0x60, [2, 3, 0, 0]],
    [8, 11, 0x18, [1, 8, 0, 0]], [8, 11, 0x10, [253, 4, 0, 0]],
    [9, 5, 0x20, [2]], [9, 5, 0x50, [100, 0, 0, 0]],
  ]);
  await expect(page.getByTestId("nav-section-firmware")).toHaveCount(0);
  await page.evaluate(() => { window.location.hash = "#/firmware"; });
  await expect(page.getByTestId("component-firmware-unavailable")).toContainText("SC-EM800");
  await expect(page.getByTestId("btn-fw-flash-pair")).toHaveCount(0);
});

test("staged edits block target switching and another component cannot restore a lever backup", async ({ page }) => {
  await connectComponents(page);
  await selectComponent(page, 4, "ST-6870-L");
  await stageChoice(page, "switch-a-function", 0);
  await expect(page.getByTestId("component-select").getByRole("button")).toBeDisabled();
  const blocked = await page.evaluate(async () => {
    try { await (window as any).cranklinkInstrumentController.selectComponent(7); return "accepted"; }
    catch (error) { return String(error); }
  });
  expect(blocked).toMatch(/stag|save|undo|discard/i);
  expect(await page.evaluate(() => {
    const s = (window as any).cranklinkInstrumentController.getState();
    return { slot: s.identity.slot, staged: s.stagedValues };
  })).toEqual({ slot: 4, staged: { "switch-a-function": 0 } });
  await save(page, 1);
  const leverBackup = await page.evaluate(() => (window as any).componentBackupAudits[0].backup.path);
  await selectComponent(page, 7, "BT-DN110");
  const rejected = await page.evaluate(async (backup) => {
    try { await (window as any).cranklinkInstrumentController.restoreBackupFiles([backup]); return "accepted"; }
    catch (error) { return String(error); }
  }, leverBackup);
  expect(rejected).toMatch(/model|serial|target|component/i);
  expect(await registers(page, 7)).toEqual({ shiftLimitUp: 255, shiftLimitDown: 3 });
  expect(await page.evaluate(() => (window as any).estepsDeviceSimulator.componentWrites.length)).toBe(1);
});
