/**
 * instrumentApp.ts - Native Custom Element (`<esteps-instrument>`) rendering the
 * complete Field Instrument UI without any VDOM or external framework dependencies.
 *
 * Uses focus-preserving DOM updates and event delegation for crisp, zero-lag
 * stepping and immediate inline diff display.
 */

import type {
  InstrumentState,
  EnrichedSetting,
  SettingSectionId,
  SimulatedModel,
  FaultInjectionType,
} from "../types.ts";
import type { SessionController } from "../core/sessionController.ts";
import { PARAMETER_GROUPS, formatSettingValue } from "../domain/schema.ts";
import { formatVersion } from "../domain/firmwarePackage.ts";

function escapeHtml(text: unknown): string {
  return String(text ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export class EstepsInstrumentElement extends HTMLElement {
  private controller!: SessionController;
  private unsubscribe: (() => void) | null = null;
  private fileInputEl: HTMLInputElement | null = null;
  private handleDocumentClick: ((e: MouseEvent) => void) | null = null;
  private consoleDraft = "";

  public bindController(controller: SessionController): void {
    this.controller = controller;
    if (this.unsubscribe) this.unsubscribe();
    this.unsubscribe = this.controller.subscribe((state) => this.render(state));
  }

  connectedCallback(): void {
    this.addEventListener("click", (e) => this.handleClick(e));
    this.addEventListener("change", (e) => this.handleChange(e));
    this.addEventListener("input", (e) => this.handleInput(e));
    this.addEventListener("submit", (e) => this.handleSubmit(e));
    this.addEventListener("dragover", (e) => {
      if ((e.target as HTMLElement)?.closest("[data-dropzone]")) {
        e.preventDefault();
      }
    });
    this.addEventListener("drop", (e) => {
      if ((e.target as HTMLElement)?.closest("[data-dropzone]")) {
        e.preventDefault();
        if (e.dataTransfer?.files?.length) {
          this.controller.importFirmwarePackageFiles(e.dataTransfer.files);
        }
      }
    });

    this.handleDocumentClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest(".more-menu")) {
        const openMenus = this.querySelectorAll<HTMLDetailsElement>(".more-menu[open]");
        openMenus.forEach((m) => m.removeAttribute("open"));
      }
    };
    document.addEventListener("click", this.handleDocumentClick);

    if (this.controller) {
      this.render(this.controller.getState());
    }
  }

  disconnectedCallback(): void {
    if (this.unsubscribe) {
      this.unsubscribe();
      this.unsubscribe = null;
    }
    if (this.handleDocumentClick) {
      document.removeEventListener("click", this.handleDocumentClick);
      this.handleDocumentClick = null;
    }
  }

  private isBusy(state: InstrumentState): boolean {
    return (
      state.phase === "connecting" ||
      state.phase === "probing" ||
      state.phase === "reading" ||
      state.phase === "committing" ||
      state.phase === "flashing"
    );
  }

  private handleClick(e: MouseEvent): void {
    const target = (e.target as HTMLElement)?.closest<HTMLElement>("[data-action]");
    if (!target || !this.controller) return;
    if ((target as HTMLButtonElement).disabled) return;

    const action = target.dataset.action;

    switch (action) {
      case "connect-usb":
        this.controller.connectAndSync("usb").catch(() => {});
        break;
      case "connect-serial":
        this.controller.connectAndSync("serial").catch(() => {});
        break;
      case "connect-ble":
        this.controller.connectAndSync("ble").catch(() => {});
        break;
      case "connect-sim": {
        const menu = this.querySelector<HTMLDetailsElement>(".more-menu");
        if (menu) menu.removeAttribute("open");
        const modelSelect = this.querySelector<HTMLSelectElement>('[data-role="sim-model"]');
        const faultSelect = this.querySelector<HTMLSelectElement>('[data-role="sim-fault"]');
        const model = (modelSelect?.value || this.controller.getState().simulatorOptions.model) as SimulatedModel;
        const fault = (faultSelect?.value || this.controller.getState().simulatorOptions.fault || "none") as FaultInjectionType;
        this.controller.connectAndSync("sim", { model, fault }).catch(() => {});
        break;
      }
      case "disconnect":
        this.controller.disconnect().catch(() => {});
        break;
      case "refresh-parameters":
        this.controller.readAllParameters().catch(() => {});
        break;
      case "select-section":
        if (target.dataset.section) {
          this.controller.setSection(target.dataset.section as any);
        }
        break;
      case "select-group":
        if (target.dataset.group) {
          this.controller.setSection("parameters");
          this.controller.setParameterGroup(target.dataset.group as any);
        }
        break;
      case "select-profile-tab":
        if (target.dataset.profile) {
          this.controller.setProfileTab(Number(target.dataset.profile) as 1 | 2);
        }
        break;
      case "step-param": {
        const name = target.dataset.name;
        const delta = Number(target.dataset.delta || 0);
        if (!name || isNaN(delta)) return;
        const state = this.controller.getState();
        const spec = state.schema.find((s) => s.name === name);
        if (!spec) return;
        const currentVal = Number(
          state.stagedValues[name] !== undefined
            ? state.stagedValues[name]
            : state.liveValues[name]?.value ?? spec.userMin,
        );
        const rawNext = Number((currentVal + delta).toFixed(4));
        const clamped = Math.max(spec.userMin, Math.min(spec.userMax, rawNext));
        this.controller.stageParameter(name, clamped);
        break;
      }
      case "select-choice": {
        const name = target.dataset.name;
        const val = target.dataset.value;
        if (!name || val === undefined) return;
        const numVal = Number(val);
        this.controller.stageParameter(name, isNaN(numVal) ? val : numVal);
        break;
      }
      case "revert-param": {
        const name = target.dataset.name;
        if (name) this.controller.revertParameter(name);
        break;
      }
      case "discard-all":
        this.controller.discardAllStaged();
        break;
      case "review-changes": {
        const rail = this.querySelector<HTMLElement>(".transaction-rail");
        rail?.scrollIntoView({ block: "start" });
        rail?.focus({ preventScroll: true });
        break;
      }
      case "commit-batch":
        this.controller.commitStagedBatch().catch(() => {});
        break;
      case "apply-macro": {
        const macro = target.dataset.macro as "eu_commute" | "trail_85nm" | "cargo_utility";
        if (macro) this.controller.applyPresetMacro(macro);
        break;
      }
      case "capture-snapshot":
        this.controller.captureManualSnapshot().catch(() => {});
        break;
      case "restore-snapshot": {
        const filename = target.dataset.filename;
        if (filename) this.controller.restoreBackupFile(filename).catch(() => {});
        break;
      }
      case "download-file": {
        const filename = target.dataset.filename;
        if (filename) this.controller.downloadVirtualFile(filename);
        break;
      }
      case "browse-firmware": {
        const input = this.querySelector<HTMLInputElement>('[data-role="firmware-file-input"]');
        input?.click();
        break;
      }
      case "fw-dry-run-pair":
        this.controller.executeFirmwareSync(false, null).catch(() => {});
        break;
      case "fw-flash-pair":
        this.controller.executeFirmwareSync(true, null).catch(() => {});
        break;
      case "fw-dry-run-single": {
        const chip = target.dataset.chip as "dcas" | "rx";
        this.controller.executeFirmwareSync(false, chip).catch(() => {});
        break;
      }
      case "fw-flash-single": {
        const chip = target.dataset.chip as "dcas" | "rx";
        this.controller.executeFirmwareSync(true, chip).catch(() => {});
        break;
      }
      case "toggle-console":
        this.controller.toggleConsole();
        break;
      case "mcp-approve":
        this.controller.getState().mcpApproval?.resolve(true);
        break;
      case "mcp-reject":
        this.controller.getState().mcpApproval?.resolve(false);
        break;
    }
  }

  private handleChange(e: Event): void {
    const target = e.target as HTMLElement;
    if (!target || !this.controller) return;

    if (target.dataset.role === "parameter-group") {
      this.controller.setSection("parameters");
      this.controller.setParameterGroup((target as HTMLSelectElement).value as SettingSectionId);
      return;
    }

    if (target.dataset.role === "sim-model") {
      const model = (target as HTMLSelectElement).value as SimulatedModel;
      this.controller.setSimulatorConfig({ model });
      return;
    }

    if (target.dataset.role === "sim-fault") {
      const fault = (target as HTMLSelectElement).value as FaultInjectionType;
      this.controller.setSimulatorConfig({ fault });
      return;
    }

    if (target.dataset.role === "param-input") {
      const name = target.dataset.name;
      const valStr = (target as HTMLInputElement).value.trim();
      if (!name) return;
      const spec = this.controller.getState().schema.find((s) => s.name === name);
      if (!spec) return;
      if (spec.value_type === "number") {
        const n = Number(valStr);
        if (!isNaN(n)) {
          const clamped = Math.max(spec.userMin, Math.min(spec.userMax, n));
          this.controller.stageParameter(name, clamped);
        }
      } else {
        this.controller.stageParameter(name, valStr);
      }
      return;
    }

    if (target.dataset.role === "firmware-file-input") {
      const files = (target as HTMLInputElement).files;
      if (files && files.length > 0) {
        this.controller.importFirmwarePackageFiles(files).catch(() => {});
      }
      return;
    }
  }

  private handleInput(e: Event): void {
    const target = e.target as HTMLElement;
    if (target?.dataset.role === "search-filter") {
      this.controller.setSearchFilter((target as HTMLInputElement).value);
    }
  }

  private handleSubmit(e: Event): void {
    const form = (e.target as HTMLElement)?.closest<HTMLFormElement>('[data-role="console-form"]');
    if (form) {
      e.preventDefault();
      const input = form.querySelector<HTMLInputElement>('[data-role="console-input"]');
      if (input && input.value.trim()) {
        const cmd = input.value.trim();
        input.value = "";
        this.controller.runRawCommand(cmd).catch(() => {});
      }
    }
  }

  private render(state: InstrumentState): void {
    // Keep mobile keyboards and unfinished console commands stable during updates.
    const activeEl = document.activeElement as HTMLInputElement | HTMLSelectElement | null;
    const activeRole = activeEl?.dataset?.role;
    const preserveFocus = ["search-filter", "console-input", "parameter-group", "sim-model", "sim-fault"].includes(activeRole || "");
    const selectionStart = preserveFocus && activeEl instanceof HTMLInputElement ? activeEl.selectionStart : null;
    const selectionEnd = preserveFocus && activeEl instanceof HTMLInputElement ? activeEl.selectionEnd : null;
    const existingConsoleInput = this.querySelector<HTMLInputElement>('[data-role="console-input"]');
    if (existingConsoleInput) this.consoleDraft = existingConsoleInput.value;

    const stagedCount = Object.keys(state.stagedValues).length;
    const busy = this.isBusy(state);
    const isMoreMenuOpen = !!this.querySelector<HTMLDetailsElement>(".more-menu[open]");

    this.innerHTML = `
      ${this.renderHeader(state, busy)}
      ${this.renderConnectionBar(state, busy, isMoreMenuOpen)}
      ${this.renderBanners(state)}
      <div class="workspace-shell ${stagedCount > 0 ? "has-staged" : ""}">
        ${this.renderSidebar(state, stagedCount)}
        <main class="workspace-main">
          ${
            state.activeSection === "parameters"
              ? this.renderParametersView(state, busy)
              : state.activeSection === "firmware"
                ? this.renderFirmwareView(state, busy)
                : this.renderJournalView(state, busy)
          }
        </main>
        ${stagedCount > 0 ? this.renderTransactionRail(state, busy) : ""}
      </div>
      ${this.renderConsoleDock(state)}
      ${stagedCount > 0 ? `
        <button type="button" class="mobile-review-dock btn btn-amber"
          data-action="review-changes" data-testid="btn-review-changes">
          Review ${stagedCount} staged change${stagedCount > 1 ? "s" : ""}
        </button>
      ` : ""}
    `;

    const consoleInput = this.querySelector<HTMLInputElement>('[data-role="console-input"]');
    if (consoleInput) consoleInput.value = this.consoleDraft;
    if (preserveFocus) {
      const nextInput = this.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-role="${activeRole}"]`);
      if (nextInput) {
        nextInput.focus({ preventScroll: true });
        if (nextInput instanceof HTMLInputElement && selectionStart !== null && selectionStart !== undefined) {
          nextInput.setSelectionRange(selectionStart, selectionEnd ?? selectionStart);
        }
      }
    }
  }

  private renderHeader(state: InstrumentState, busy: boolean): string {
    const isConnected = !!state.identity;
    const badgeClass = busy
      ? "busy"
      : state.transport === "sim"
        ? "sim"
        : isConnected
          ? "connected"
          : "";

    const badgeText = busy
      ? state.phase.toUpperCase()
      : isConnected
        ? `${state.transport === "sim" ? "SIM" : state.transport?.toUpperCase()}: ${state.identity!.model}`
        : "DISCONNECTED";

    return `
      <header class="instrument-header">
        <div class="header-brand">
          <span class="brand-mark">ESTEPS</span>
          <span class="brand-subtitle">Drive Unit Field Instrument</span>
        </div>

        <div class="header-telemetry">
          <span class="status-badge ${badgeClass}" data-testid="header-status-badge">
            <span class="status-dot"></span>
            <span>${escapeHtml(badgeText)}</span>
          </span>

          ${
            state.identity
              ? `
            <div class="identity-strip" data-testid="identity-strip">
              <span>SN: <strong>${escapeHtml(state.identity.serial)}</strong></span>
              <span>DCAS: <strong>${escapeHtml(state.identity.dcasVersion)}</strong></span>
              <span>RX: <strong>${escapeHtml(state.identity.rxVersion)}</strong></span>
              <span>Profile: <strong>${state.identity.isCargo ? "Cargo" : "Standard"}</strong></span>
            </div>
          `
              : ""
          }
        </div>
      </header>
    `;
  }

  private renderConnectionBar(state: InstrumentState, busy: boolean, isMoreMenuOpen = false): string {
    const isConnected = !!state.identity;
    const isBrave = typeof navigator !== "undefined" && Boolean((navigator as any).brave);
    const available = {
      ble: "bluetooth" in navigator && !isBrave,
      usb: "usb" in navigator,
      serial: "serial" in navigator,
    };
    const unavailable = [
      !available.ble && (isBrave ? "Bluetooth (disabled in Brave)" : "Bluetooth"),
      !available.usb && "USB",
      !available.serial && "Serial",
    ].filter(Boolean);

    return `
      <section class="connection-bar" aria-label="Adapter Connection">
        <div class="connection-grid">
          <div class="transport-actions">
            <button
              type="button"
              class="btn ${state.transport === "ble" ? "btn-primary" : ""}"
              data-action="connect-ble"
              data-testid="btn-connect-ble"
              title="${available.ble ? "Connect a Bluetooth gateway" : isBrave ? "Web Bluetooth is disabled globally in Brave (use Chrome/Edge or enable brave://flags/#enable-web-bluetooth)" : "Web Bluetooth is unavailable in this browser"}"
              ${busy || !available.ble ? "disabled" : ""}
            >
              Connect Bluetooth
            </button>
            <button
              type="button"
              class="btn ${state.transport === "usb" ? "btn-primary" : ""}"
              data-action="connect-usb"
              data-testid="btn-connect-usb"
              title="${available.usb ? "Connect an SM-PCE02 adapter" : "WebUSB is unavailable in this browser"}"
              ${busy || !available.usb ? "disabled" : ""}
            >
              Connect USB (SM-PCE02)
            </button>
            <button
              type="button"
              class="btn ${state.transport === "serial" ? "btn-primary" : ""}"
              data-action="connect-serial"
              data-testid="btn-connect-serial"
              title="${available.serial ? "Connect a serial adapter" : "Web Serial is unavailable in this browser"}"
              ${busy || !available.serial ? "disabled" : ""}
            >
              Connect Serial
            </button>

            <details class="more-menu" data-testid="more-menu" ${isMoreMenuOpen ? "open" : ""}>
              <summary
                class="btn"
                data-testid="btn-more-menu"
                title="More options (Simulator)"
                aria-label="More options"
                ${busy ? "disabled" : ""}
              >
                ⋯
              </summary>
              <div class="menu-dropdown">
                <div class="menu-header">In-Browser Simulator</div>
                <div class="menu-field">
                  <label for="sim-model-select">Target Motor</label>
                  <select
                    id="sim-model-select"
                    class="select-input select-sm"
                    data-role="sim-model"
                    data-testid="sim-model-select"
                    aria-label="Simulated Model"
                    ${busy ? "disabled" : ""}
                  >
                    <option value="ep800-crg" ${state.simulatorOptions.model === "ep800-crg" ? "selected" : ""}>DU-EP800-CRG (Cargo)</option>
                    <option value="ep600" ${state.simulatorOptions.model === "ep600" ? "selected" : ""}>DU-EP600 (Gen 2)</option>
                    <option value="ep801" ${state.simulatorOptions.model === "ep801" ? "selected" : ""}>DU-EP801 (Flagship)</option>
                  </select>
                </div>

                <div class="menu-field">
                  <label for="sim-fault-select">Bus Emulation</label>
                  <select
                    id="sim-fault-select"
                    class="select-input select-sm"
                    data-role="sim-fault"
                    data-testid="sim-fault-select"
                    aria-label="Simulated Fault Mode"
                    ${busy ? "disabled" : ""}
                  >
                    <option value="none" ${state.simulatorOptions.fault === "none" ? "selected" : ""}>Healthy Bus</option>
                    <option value="auth_reject" ${state.simulatorOptions.fault === "auth_reject" ? "selected" : ""}>Fault: Auth Reject</option>
                    <option value="disconnect_on_write" ${state.simulatorOptions.fault === "disconnect_on_write" ? "selected" : ""}>Fault: Disconnect on Write</option>
                    <option value="readonly_rejection" ${state.simulatorOptions.fault === "readonly_rejection" ? "selected" : ""}>Fault: Write Rejected</option>
                  </select>
                </div>

                <div class="menu-footer">
                  <button
                    type="button"
                    class="btn btn-sm ${state.transport === "sim" ? "btn-primary" : ""}"
                    data-action="connect-sim"
                    data-testid="btn-connect-sim"
                    ${busy ? "disabled" : ""}
                  >
                    ${state.transport === "sim" ? "Reconnect Simulator" : "Start Simulator"}
                  </button>
                </div>
              </div>
            </details>
          </div>

          <div class="connection-status">
            <span style="font-size: 12px; color: var(--text-secondary);" data-testid="status-message">
              ${escapeHtml(state.statusMessage)}
            </span>
            ${
              isConnected
                ? `
              <button
                type="button"
                class="btn btn-sm"
                data-action="refresh-parameters"
                data-testid="btn-refresh-parameters"
                ${busy ? "disabled" : ""}
              >
                Re-read All
              </button>
              <button
                type="button"
                class="btn btn-sm"
                data-action="disconnect"
                data-testid="btn-disconnect"
                ${busy ? "disabled" : ""}
              >
                Disconnect
              </button>
            `
                : ""
            }
          </div>
        </div>
        ${unavailable.length ? `<p class="browser-support-note">${unavailable.join(", ")} device access is unavailable in this browser. Use a browser with the required device API, or try the simulator.</p>` : ""}
      </section>
    `;
  }

  private renderBanners(state: InstrumentState): string {
    let html = "";

    if (state.errorBanner) {
      html += `
        <div class="banner banner-error" role="alert" data-testid="error-banner">
          <div><strong>Operation Error:</strong> ${escapeHtml(state.errorBanner)}</div>
        </div>
      `;
    }

    if (state.mcpApproval) {
      const req = state.mcpApproval;
      const diffSummary = req.diff
        .map((d) => `${escapeHtml(d.label)}: ${escapeHtml(d.before)} → ${escapeHtml(d.after)}`)
        .join("; ");
      html += `
        <div class="banner banner-mcp" role="region" aria-label="Agent Approval Request" data-testid="mcp-approval-banner">
          <div>
            <strong>External Agent Request (${escapeHtml(req.tool)}):</strong>
            <span>${diffSummary || "Execute requested operation"}</span>
          </div>
          <div style="display: flex; gap: 6px; flex-shrink: 0;">
            <button type="button" class="btn btn-sm" data-action="mcp-reject" data-testid="btn-mcp-reject">Reject</button>
            <button type="button" class="btn btn-sm btn-amber" data-action="mcp-approve" data-testid="btn-mcp-approve">Authorize Write</button>
          </div>
        </div>
      `;
    }

    return html;
  }

  private renderSidebar(state: InstrumentState, stagedCount: number): string {
    const counts: Record<string, number> = { all: state.schema.length };
    for (const g of PARAMETER_GROUPS) {
      counts[g.id] = state.schema.filter((s) => s.section === g.id).length;
    }

    return `
      <aside class="nav-rail" aria-label="Workspace Navigation">
        <div class="nav-group parameter-navigation">
          <div class="nav-group-header">Parameter Groups</div>
          <label class="mobile-group-picker" for="mobile-parameter-group">Parameter group
            <select id="mobile-parameter-group" class="select-input"
              data-role="parameter-group" data-testid="mobile-parameter-group">
              ${state.activeSection !== "parameters" ? '<option value="" selected disabled>Choose parameter group…</option>' : ""}
              ${[...PARAMETER_GROUPS, { id: "all", title: "All Parameters" }].filter((g) =>
                g.id === "all" || !state.schema.length || counts[g.id] > 0).map((g) => `
                <option value="${g.id}" ${state.activeSection === "parameters" && state.activeParameterGroup === g.id ? "selected" : ""}>${escapeHtml(g.title)} (${counts[g.id] || 0})</option>
              `).join("")}
            </select>
          </label>
          <div class="nav-list">
            ${PARAMETER_GROUPS.map((g) => {
              const count = counts[g.id] || 0;
              if (count === 0 && state.schema.length > 0) return "";
              const isActive =
                state.activeSection === "parameters" && state.activeParameterGroup === g.id;
              return `
                <button
                  type="button"
                  class="nav-item ${isActive ? "active" : ""}"
                  data-action="select-group"
                  data-group="${g.id}"
                  data-testid="nav-group-${g.id}"
                >
                  <span>${escapeHtml(g.title)}</span>
                  <span class="nav-count">${count}</span>
                </button>
              `;
            }).join("")}
            <button
              type="button"
              class="nav-item ${state.activeSection === "parameters" && state.activeParameterGroup === "all" ? "active" : ""}"
              data-action="select-group"
              data-group="all"
              data-testid="nav-group-all"
            >
              <span>All Parameters</span>
              <span class="nav-count">${counts.all}</span>
            </button>
          </div>
        </div>

        <div class="nav-group system-navigation">
          <div class="nav-group-header">System Operations</div>
          <div class="nav-list">
            <button
              type="button"
              class="nav-item ${state.activeSection === "firmware" ? "active" : ""}"
              data-action="select-section"
              data-section="firmware"
              data-testid="nav-section-firmware"
            >
              <span>Dual-MCU Firmware Sync</span>
              <span class="nav-count">${state.firmware.companionValid ? "Ready" : "Pair"}</span>
            </button>
            <button
              type="button"
              class="nav-item ${state.activeSection === "journal" ? "active" : ""}"
              data-action="select-section"
              data-section="journal"
              data-testid="nav-section-journal"
            >
              <span>Snapshots & Rollback</span>
              <span class="nav-count">${state.snapshots.length}</span>
            </button>
          </div>
        </div>

        ${
          stagedCount > 0
            ? `
          <div style="font-size: 12px; color: var(--accent-amber); font-weight: 600; padding: 4px 2px;">
            ${stagedCount} uncommitted change${stagedCount > 1 ? "s" : ""} staged
          </div>
        `
            : ""
        }
      </aside>
    `;
  }

  private renderParametersView(state: InstrumentState, busy: boolean): string {
    if (!state.identity) {
      const hasBluetooth = "bluetooth" in navigator;
      const hasUsb = "usb" in navigator;
      return `
        <div class="section-card" style="padding: 32px 20px; text-align: center;">
          <h2 style="font-size: 16px; font-weight: 600; margin-bottom: 6px;">No Drive Unit Connected</h2>
          <p style="font-size: 13px; color: var(--text-secondary); max-width: 520px; margin: 0 auto 16px;">
            Connect a Shimano Bluetooth gateway, an SM-PCE02 USB adapter, or a serial diagnostic cable to inspect and configure the drive unit.
          </p>
          <div class="empty-state-actions">
            <button
              type="button"
              class="btn btn-primary"
              data-action="connect-ble"
              title="${hasBluetooth ? "Connect a Bluetooth gateway" : "Web Bluetooth is unavailable in this browser"}"
              ${busy || !hasBluetooth ? "disabled" : ""}
            >
              Connect Bluetooth
            </button>
            <button
              type="button"
              class="btn"
              data-action="connect-usb"
              title="${hasUsb ? "Connect an SM-PCE02 adapter" : "WebUSB is unavailable in this browser"}"
              ${busy || !hasUsb ? "disabled" : ""}
            >
              Connect USB
            </button>
          </div>
          <div style="margin-top: 18px; padding-top: 14px; border-top: 1px dashed var(--border-hairline);">
            <button
              type="button"
              class="btn-subtle-link"
              data-action="connect-sim"
              data-testid="empty-state-start-sim"
              ${busy ? "disabled" : ""}
            >
              Demo: Launch in-browser wire simulator (${escapeHtml(state.simulatorOptions.model.toUpperCase())}) →
            </button>
          </div>
        </div>
      `;
    }

    const query = state.searchFilter.trim().toLowerCase();
    const groupsToRender =
      state.activeParameterGroup === "all"
        ? PARAMETER_GROUPS
        : PARAMETER_GROUPS.filter((g) => g.id === state.activeParameterGroup);

    return `
      <div class="parameter-toolbar">
        <div class="preset-strip">
          <span class="preset-strip-label">Quick Stage Presets:</span>
          <button
            type="button"
            class="btn btn-sm"
            data-action="apply-macro"
            data-macro="eu_commute"
            data-testid="macro-eu-commute"
            ${busy ? "disabled" : ""}
          >
            EU Commute (25 km/h · 2280 mm · Lights On)
          </button>
          <button
            type="button"
            class="btn btn-sm"
            data-action="apply-macro"
            data-macro="trail_85nm"
            data-testid="macro-trail-85nm"
            ${busy ? "disabled" : ""}
          >
            Trail Climbing (85 Nm Boost)
          </button>
          <button
            type="button"
            class="btn btn-sm"
            data-action="apply-macro"
            data-macro="cargo_utility"
            data-testid="macro-cargo-utility"
            ${busy ? "disabled" : ""}
          >
            Cargo Utility (2150 mm · 70 Nm Trail)
          </button>
        </div>

        <input
          type="search"
          class="text-input"
          placeholder="Filter parameters..."
          value="${escapeHtml(state.searchFilter)}"
          data-role="search-filter"
          data-testid="search-filter-input"
          aria-label="Filter parameters"
          autocapitalize="off"
          spellcheck="false"
        />
      </div>

      ${groupsToRender
        .map((group) => {
          let items = state.schema.filter((s) => s.section === group.id);
          if (group.id === "profiles" && !query) {
            items = items.filter(
              (s) =>
                !s.name.startsWith("profile") ||
                s.name.startsWith(`profile${state.activeProfileTab}.`),
            );
          }
          if (query) {
            items = items.filter(
              (s) =>
                s.name.toLowerCase().includes(query) ||
                s.label.toLowerCase().includes(query) ||
                s.description.toLowerCase().includes(query),
            );
          }
          if (items.length === 0) return "";

          return `
            <section class="section-card" data-testid="section-${group.id}">
              <div class="section-header">
                <div>
                  <div class="section-title">${escapeHtml(group.title)}</div>
                  <div class="section-subtitle">${escapeHtml(group.subtitle)}</div>
                </div>
                ${
                  group.id === "profiles"
                    ? `
                  <div class="segmented-group" role="tablist" aria-label="Assist Profile Selector">
                    <button
                      type="button"
                      class="segmented-option ${state.activeProfileTab === 1 ? "selected" : ""}"
                      data-action="select-profile-tab"
                      data-profile="1"
                      data-testid="tab-profile-1"
                    >
                      Profile 1
                    </button>
                    <button
                      type="button"
                      class="segmented-option ${state.activeProfileTab === 2 ? "selected" : ""}"
                      data-action="select-profile-tab"
                      data-profile="2"
                      data-testid="tab-profile-2"
                    >
                      Profile 2
                    </button>
                  </div>
                `
                    : ""
                }
              </div>

              <div class="param-list">
                ${items.map((spec) => this.renderParameterRow(spec, state, busy)).join("")}
              </div>
            </section>
          `;
        })
        .join("")}
    `;
  }

  private renderParameterRow(
    spec: EnrichedSetting,
    state: InstrumentState,
    busy: boolean,
  ): string {
    const liveEntry = state.liveValues[spec.name];
    const liveVal = liveEntry?.value;
    const isStaged = state.stagedValues[spec.name] !== undefined;
    const activeVal = isStaged ? state.stagedValues[spec.name] : liveVal;

    const rangeInfo =
      spec.value_type === "number" && !spec.choiceLabels
        ? `${spec.userMin}–${spec.userMax}${spec.displayUnit ? " " + spec.displayUnit : ""} (step ${spec.userStep})`
        : spec.unit;

    return `
      <div
        class="param-row ${isStaged ? "is-staged" : ""}"
        data-testid="param-row-${spec.name}"
      >
        <div class="param-meta">
          <div class="param-label-line">
            <span class="param-label">${escapeHtml(spec.label)}</span>
            <span class="param-key">${escapeHtml(spec.name)}</span>
          </div>
          ${spec.description ? `<div class="param-desc">${escapeHtml(spec.description)}</div>` : ""}
          <div class="param-range">${escapeHtml(rangeInfo)}</div>
        </div>

        <div class="param-live">
          <div>Live: <strong>${escapeHtml(formatSettingValue(spec, liveVal))}</strong></div>
          ${
            isStaged
              ? `
            <div class="param-diff-badge">
              <span>Staged: ${escapeHtml(formatSettingValue(spec, activeVal))}</span>
              <button
                type="button"
                class="btn btn-sm"
                data-action="revert-param"
                data-name="${escapeHtml(spec.name)}"
                data-testid="revert-${spec.name}"
              >
                Revert
              </button>
            </div>
          `
              : ""
          }
        </div>

        <div class="param-control-cell">
          ${this.renderParameterControl(spec, activeVal, busy)}
        </div>
      </div>
    `;
  }

  private renderParameterControl(
    spec: EnrichedSetting,
    activeVal: number | string | null | undefined,
    busy: boolean,
  ): string {
    // 1. Choice / Enum / Boolean Segmented Control
    if (spec.choiceLabels || (spec.choices && spec.choices.length > 0)) {
      const options =
        spec.choices && spec.choices.length > 0
          ? spec.choices
          : Object.keys(spec.choiceLabels || {}).map(Number);

      return `
        <div class="segmented-group" data-testid="control-${spec.name}">
          ${options
            .map((opt) => {
              const label = spec.choiceLabels?.[opt] ?? String(opt);
              const selected = Number(activeVal) === opt;
              return `
                <button
                  type="button"
                  class="segmented-option ${selected ? "selected" : ""}"
                  data-action="select-choice"
                  data-name="${escapeHtml(spec.name)}"
                  data-value="${opt}"
                  data-testid="choice-${spec.name}-${opt}"
                  ${busy ? "disabled" : ""}
                >
                  ${escapeHtml(label)}
                </button>
              `;
            })
            .join("")}
        </div>
      `;
    }

    // 2. String or Date input
    if (spec.value_type === "string" || spec.value_type === "date") {
      return `
        <input
          type="text"
          class="text-input tabular"
          value="${escapeHtml(activeVal ?? "")}"
          placeholder="${spec.value_type === "date" ? "YYYY-MM-DD or unset" : "ASCII text"}"
          data-role="param-input"
          data-name="${escapeHtml(spec.name)}"
          data-testid="input-${spec.name}"
          ${busy ? "disabled" : ""}
        />
      `;
    }

    // 3. Numeric Stepper + Direct Input + Optional Presets
    const numericVal = activeVal !== null && activeVal !== undefined ? Number(activeVal) : spec.userMin;
    // Use 10mm ergonomic step button for tire-circumference while keeping 1mm direct input resolution
    const buttonStep = spec.name === "tire-circumference" ? 10 : spec.userStep;

    return `
      <div class="stepper-group">
        <button
          type="button"
          class="stepper-btn"
          aria-label="Decrease ${escapeHtml(spec.label)}"
          data-action="step-param"
          data-name="${escapeHtml(spec.name)}"
          data-delta="${-buttonStep}"
          data-testid="stepper-minus-${spec.name}"
          ${busy ? "disabled" : ""}
        >
          −
        </button>
        <div class="stepper-input-wrap">
          <input
            type="number"
            inputmode="decimal"
            class="stepper-input"
            min="${spec.userMin}"
            max="${spec.userMax}"
            step="${spec.userStep}"
            value="${isNaN(numericVal) ? "" : numericVal}"
            data-role="param-input"
            data-name="${escapeHtml(spec.name)}"
            data-testid="input-${spec.name}"
            aria-label="${escapeHtml(spec.label)}"
            ${busy ? "disabled" : ""}
          />
          ${spec.displayUnit ? `<span class="stepper-unit">${escapeHtml(spec.displayUnit)}</span>` : ""}
        </div>
        <button
          type="button"
          class="stepper-btn"
          aria-label="Increase ${escapeHtml(spec.label)}"
          data-action="step-param"
          data-name="${escapeHtml(spec.name)}"
          data-delta="${buttonStep}"
          data-testid="stepper-plus-${spec.name}"
          ${busy ? "disabled" : ""}
        >
          +
        </button>
      </div>
      ${
        spec.presets && spec.presets.length > 0
          ? `
        <div class="preset-pills">
          ${spec.presets
            .map(
              (p) => `
            <button
              type="button"
              class="preset-pill ${String(activeVal) === String(p.value) ? "active" : ""}"
              data-action="select-choice"
              data-name="${escapeHtml(spec.name)}"
              data-value="${escapeHtml(p.value)}"
              data-testid="preset-${spec.name}-${p.value}"
              ${busy ? "disabled" : ""}
            >
              ${escapeHtml(p.label)}
            </button>
          `,
            )
            .join("")}
        </div>
      `
          : ""
      }
    `;
  }

  private renderTransactionRail(state: InstrumentState, busy: boolean): string {
    const entries = Object.entries(state.stagedValues);

    return `
      <aside class="transaction-rail" aria-label="Staged Transaction" tabindex="-1" data-testid="transaction-rail">
        <div class="transaction-header">
          <span class="transaction-title">Staged Changes (${entries.length})</span>
          <button
            type="button"
            class="btn btn-sm"
            data-action="discard-all"
            data-testid="btn-discard-all"
            ${busy ? "disabled" : ""}
          >
            Discard All
          </button>
        </div>

        <div class="transaction-list">
          ${entries
            .map(([name, targetVal]) => {
              const spec = state.schema.find((s) => s.name === name);
              const label = spec?.label || name;
              const liveVal = state.liveValues[name]?.value;
              const beforeStr = spec ? formatSettingValue(spec, liveVal) : String(liveVal ?? "—");
              const afterStr = spec ? formatSettingValue(spec, targetVal) : String(targetVal);

              return `
                <div class="transaction-item" data-testid="tx-item-${name}">
                  <div>
                    <div style="font-weight: 600;">${escapeHtml(label)}</div>
                    <div class="tabular" style="color: var(--text-secondary); font-size: 11px;">
                      ${escapeHtml(beforeStr)} → <strong>${escapeHtml(afterStr)}</strong>
                    </div>
                  </div>
                  <button
                    type="button"
                    class="btn btn-sm"
                    data-action="revert-param"
                    data-name="${escapeHtml(name)}"
                    aria-label="Revert ${escapeHtml(label)}"
                    ${busy ? "disabled" : ""}
                  >
                    ×
                  </button>
                </div>
              `;
            })
            .join("")}
        </div>

        <div class="transaction-footer">
          <div style="font-size: 11px; color: var(--text-secondary);">
            Executes in a single authenticated session with automatic pre-write backups and readback verification.
          </div>
          <button
            type="button"
            class="btn btn-amber"
            style="width: 100%;"
            data-action="commit-batch"
            data-testid="btn-commit-batch"
            ${busy ? "disabled" : ""}
          >
            Write ${entries.length} Change${entries.length > 1 ? "s" : ""} to Drive Unit
          </button>
        </div>
      </aside>
    `;
  }

  /**
   * Synchronized Dual-MCU Firmware View:
   * Defaults to flashing both companion chips (DCAS + RX) together as one verified package.
   * Individual single-chip flashing is hidden inside an explicit "Single-MCU Recovery Override" disclosure.
   */
  private renderFirmwareView(state: InstrumentState, busy: boolean): string {
    const fw = state.firmware;
    const dcas = fw.dcasFile;
    const rx = fw.rxFile;
    const canCheck = !!state.identity && fw.companionValid && !busy;
    const canFlash =
      canCheck && fw.dryRunPassed && fw.dryRunSerial === state.identity?.serial;

    return `
      <section class="section-card" data-testid="firmware-section">
        <div class="section-header">
          <div>
            <div class="section-title">Synchronized Dual-MCU Firmware Package (DCAS + RX)</div>
            <div class="section-subtitle">
              Shimano STEPS drive units require matched DCAS (motor control) and RX (communication) companion firmware. Both processors are validated and synchronized together by default.
            </div>
          </div>
        </div>

        <div style="padding: 16px;">
          <div
            class="firmware-dropzone"
            data-dropzone="true"
            data-action="browse-firmware"
            data-testid="firmware-dropzone"
          >
            <input
              type="file"
              accept=".dat,.bin"
              multiple
              style="display: none;"
              data-role="firmware-file-input"
              data-testid="firmware-file-input"
            />
            <div style="font-weight: 600; font-size: 14px; margin-bottom: 4px;">
              Select or Drop Matched Firmware Pair (.dat)
            </div>
            <div style="font-size: 12px; color: var(--text-secondary);">
              Drop both <code>DUEP800-D.*.dat</code> (DCAS) and <code>DUEP800-M.*.dat</code> (RX) files together for automatic companion verification.
            </div>
          </div>

          <div class="firmware-slots-grid">
            <div class="firmware-slot-card ${dcas && !dcas.error ? "loaded" : ""}" data-testid="slot-dcas">
              <div style="font-size: 11px; font-weight: 700; text-transform: uppercase; color: var(--text-muted); margin-bottom: 4px;">
                Processor 1: DCAS (Cortex D-Image)
              </div>
              ${
                dcas
                  ? `
                <div class="tabular" style="font-weight: 600; font-size: 13px;">${escapeHtml(dcas.filename)}</div>
                <div class="tabular" style="font-size: 12px; color: var(--text-secondary); margin-top: 2px;">
                  Version: <strong>${escapeHtml(formatVersion(dcas.metadata?.version))}</strong> · ${(dcas.sizeBytes / 1024).toFixed(1)} KiB
                </div>
                <div class="tabular" style="font-size: 11px; color: var(--text-muted); margin-top: 2px;">
                  SHA-256: ${escapeHtml(dcas.sha256.slice(0, 16))}…
                </div>
              `
                  : `<div style="font-size: 12px; color: var(--text-muted);">No DCAS (-D) file loaded</div>`
              }
            </div>

            <div class="firmware-slot-card ${rx && !rx.error ? "loaded" : ""}" data-testid="slot-rx">
              <div style="font-size: 11px; font-weight: 700; text-transform: uppercase; color: var(--text-muted); margin-bottom: 4px;">
                Processor 2: RX (Renesas M-Image)
              </div>
              ${
                rx
                  ? `
                <div class="tabular" style="font-weight: 600; font-size: 13px;">${escapeHtml(rx.filename)}</div>
                <div class="tabular" style="font-size: 12px; color: var(--text-secondary); margin-top: 2px;">
                  Version: <strong>${escapeHtml(formatVersion(rx.metadata?.version))}</strong> · ${(rx.sizeBytes / 1024).toFixed(1)} KiB
                </div>
                <div class="tabular" style="font-size: 11px; color: var(--text-muted); margin-top: 2px;">
                  SHA-256: ${escapeHtml(rx.sha256.slice(0, 16))}…
                </div>
              `
                  : `<div style="font-size: 12px; color: var(--text-muted);">No RX (-M) file loaded</div>`
              }
            </div>
          </div>

          <div
            style="margin-top: 12px; padding: 10px 12px; border-radius: var(--radius-sm); border: 1px solid var(--border-hairline); background: ${fw.companionValid ? "var(--accent-green-bg)" : "var(--bg-subtle)"}; color: ${fw.companionValid ? "var(--accent-green)" : "var(--text-secondary)"}; font-size: 13px; font-weight: 500;"
            data-testid="companion-status"
          >
            ${escapeHtml(fw.companionMessage)}
          </div>

          <div class="firmware-steps">
            <div class="firmware-step-box">
              <div>
                <div style="font-weight: 700; font-size: 13px; margin-bottom: 4px;">
                  1. Non-Erasing Compatibility Check (Both MCUs)
                </div>
                <div style="font-size: 12px; color: var(--text-secondary);">
                  Verifies live drive unit serial (${escapeHtml(state.identity?.serial || "not connected")}), probes both DCAS and RX bootloaders without erasing flash, and confirms clean application return.
                </div>
              </div>
              <button
                type="button"
                class="btn ${fw.dryRunPassed ? "" : "btn-primary"}"
                data-action="fw-dry-run-pair"
                data-testid="btn-fw-dry-run-pair"
                ${!canCheck ? "disabled" : ""}
              >
                ${fw.dryRunPassed ? "Compatibility Check Passed (Re-run)" : "Run Dual-MCU Compatibility Check"}
              </button>
            </div>

            <div class="firmware-step-box">
              <div>
                <div style="font-weight: 700; font-size: 13px; margin-bottom: 4px;">
                  2. Program Synchronized Firmware Package (DCAS + RX)
                </div>
                <div style="font-size: 12px; color: var(--text-secondary);">
                  Exports a full settings backup, flashes DCAS and RX in sequence, verifies post-flash versions on both processors, and confirms zero setting drift.
                </div>
              </div>
              <button
                type="button"
                class="btn btn-danger"
                data-action="fw-flash-pair"
                data-testid="btn-fw-flash-pair"
                ${!canFlash ? "disabled" : ""}
              >
                Program Synchronized Package (Both MCUs)
              </button>
            </div>
          </div>

          <details style="margin-top: 18px; border-top: 1px solid var(--border-hairline); padding-top: 12px;" data-testid="single-mcu-override">
            <summary style="font-size: 12px; font-weight: 600; color: var(--text-muted); cursor: pointer;">
              Advanced: Single-MCU Recovery Override (Board-Level Recovery Only)
            </summary>
            <div style="margin-top: 10px; padding: 12px; background: var(--bg-subtle); border: 1px solid var(--border-hairline); border-radius: var(--radius-sm); font-size: 12px; color: var(--text-secondary);">
              <p style="margin-bottom: 10px;">
                Use single-MCU operations only if a previous interrupted update left one processor out of sync while its companion is already at the target version. Companion file verification is still enforced.
              </p>
              <div style="display: flex; flex-wrap: wrap; gap: 8px;">
                <button
                  type="button"
                  class="btn btn-sm"
                  data-action="fw-dry-run-single"
                  data-chip="dcas"
                  ${!canCheck ? "disabled" : ""}
                >
                  Dry-Run DCAS Only
                </button>
                <button
                  type="button"
                  class="btn btn-sm"
                  data-action="fw-flash-single"
                  data-chip="dcas"
                  ${!canFlash ? "disabled" : ""}
                >
                  Program DCAS Only
                </button>
                <button
                  type="button"
                  class="btn btn-sm"
                  data-action="fw-dry-run-single"
                  data-chip="rx"
                  ${!canCheck ? "disabled" : ""}
                >
                  Dry-Run RX Only
                </button>
                <button
                  type="button"
                  class="btn btn-sm"
                  data-action="fw-flash-single"
                  data-chip="rx"
                  ${!canFlash ? "disabled" : ""}
                >
                  Program RX Only
                </button>
              </div>
            </div>
          </details>
        </div>
      </section>
    `;
  }

  private renderJournalView(state: InstrumentState, busy: boolean): string {
    return `
      <section class="section-card" data-testid="journal-section">
        <div class="section-header">
          <div>
            <div class="section-title">Configuration Snapshots & Transaction Journal</div>
            <div class="section-subtitle">
              Every parameter write automatically stores a serial-bound pre-state backup in browser storage. You can also capture full configuration snapshots on demand.
            </div>
          </div>
          <button
            type="button"
            class="btn btn-primary btn-sm"
            data-action="capture-snapshot"
            data-testid="btn-capture-snapshot"
            ${!state.identity || busy ? "disabled" : ""}
          >
            Capture Full Snapshot
          </button>
        </div>

        <div class="param-list">
          ${
            state.snapshots.length === 0
              ? `
            <div style="padding: 24px; text-align: center; color: var(--text-muted); font-size: 13px;">
              No snapshots or transaction journals stored yet. Any setting change or manual snapshot will appear here.
            </div>
          `
              : state.snapshots
                  .map(
                    (snap) => `
              <div class="param-row" data-testid="snapshot-row-${snap.filename}">
                <div class="param-meta">
                  <div class="param-label-line">
                    <span class="param-label tabular">${escapeHtml(snap.filename)}</span>
                  </div>
                  <div class="param-desc">${escapeHtml(snap.summary)}</div>
                </div>
                <div class="param-live">
                  <span>Type: ${escapeHtml(snap.kind)}</span>
                  ${snap.serial ? `<span>SN: ${escapeHtml(snap.serial.toUpperCase())}</span>` : ""}
                </div>
                <div class="param-control-cell" style="flex-direction: row; justify-content: flex-end;">
                  <button
                    type="button"
                    class="btn btn-sm"
                    data-action="download-file"
                    data-filename="${escapeHtml(snap.filename)}"
                  >
                    Export
                  </button>
                  ${
                    snap.kind === "setting-backup"
                      ? `
                    <button
                      type="button"
                      class="btn btn-sm btn-amber"
                      data-action="restore-snapshot"
                      data-filename="${escapeHtml(snap.filename)}"
                      data-testid="btn-restore-${snap.filename}"
                      ${!state.identity || busy ? "disabled" : ""}
                    >
                      Restore Value
                    </button>
                  `
                      : ""
                  }
                </div>
              </div>
            `,
                  )
                  .join("")
          }
        </div>
      </section>
    `;
  }

  private renderConsoleDock(state: InstrumentState): string {
    return `
      <footer class="console-dock">
        <div class="console-panel">
          <button type="button"
            class="console-header"
            data-action="toggle-console"
            data-testid="console-toggle"
            aria-expanded="${state.consoleExpanded}"
          >
            <span>Diagnostic CLI & Protocol Trace (${state.logs.length} lines)</span>
            <span>${state.consoleExpanded ? "Hide ▲" : "Expand ▼"}</span>
          </button>
          ${
            state.consoleExpanded
              ? `
            <div class="console-body" data-testid="console-body">
              <div class="console-log-list">
                ${state.logs
                  .slice(-80)
                  .map(
                    (l) => `
                  <div class="console-log-line ${l.stream}">
                    <span style="opacity: 0.5;">[${escapeHtml(l.timestamp)}]</span> ${escapeHtml(l.text)}
                  </div>
                `,
                  )
                  .join("")}
              </div>
              <form class="console-input-row" data-role="console-form">
                <input
                  type="text"
                  class="console-input"
                  placeholder="Run raw cranklink command (e.g. probe, units, settings read)..."
                  data-role="console-input"
                  data-testid="console-input"
                  aria-label="CLI command"
                  autocapitalize="off"
                  autocomplete="off"
                  spellcheck="false"
                  enterkeyhint="send"
                />
                <button type="submit" class="btn btn-sm">Execute</button>
              </form>
            </div>
          `
              : ""
          }
        </div>
      </footer>
    `;
  }
}

if (typeof customElements !== "undefined") {
  if (!customElements.get("esteps-instrument")) {
    customElements.define("esteps-instrument", EstepsInstrumentElement);
  }
  if (!customElements.get("cranklink-instrument")) {
    class CranklinkInstrumentElement extends EstepsInstrumentElement {}
    customElements.define("cranklink-instrument", CranklinkInstrumentElement);
  }
}
