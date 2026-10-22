/**
 * webMcpServer.ts - In-Browser Model Context Protocol (MCP) Server for esteps Field Instrument.
 * Exposes structured tools and resources over JSON-RPC 2.0 (window.postMessage and direct API)
 * with human-in-the-loop confirmation for any write transaction.
 */

import type { SessionController } from "./sessionController.ts";

export const MCP_TOOLS = [
  {
    name: "cranklink_probe",
    description: "Read the active component identity, serial number, and available firmware versions.",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
  {
    name: "cranklink_get_schema",
    description: "Return the active component setting schema, including ranges, choices and read-only fields.",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
  {
    name: "cranklink_read_settings",
    description: "Read all live parameter values from the selected component.",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
  {
    name: "cranklink_set_setting",
    description: "Stage and apply a single setting update with human confirmation and automatic pre-write backup.",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Setting identifier (e.g. tire-circumference)" },
        value: { type: "string", description: "Target value" },
      },
      required: ["name", "value"],
    },
  },
  {
    name: "cranklink_batch_settings",
    description: "Stage and apply multiple setting changes in one session with human confirmation and pre-write backups.",
    inputSchema: {
      type: "object",
      properties: {
        operations: {
          type: "array",
          description: "Array of { op: 'set', name: string, value: string | number }",
        },
      },
      required: ["operations"],
    },
  },
  {
    name: "cranklink_create_backup",
    description: "Export a durable JSON snapshot of the active component parameters.",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
  {
    name: "cranklink_list_components",
    description: "Return the connected component inventory. Refresh when there are no staged edits or active operations.",
    inputSchema: { type: "object", properties: { refresh: { type: "boolean" } } },
  },
  {
    name: "cranklink_select_component",
    description: "Select a catalogued component from the current inventory and load its settings. Save or discard staged edits first.",
    inputSchema: { type: "object", properties: { slot: { type: "integer", minimum: 0, maximum: 31 } }, required: ["slot"] },
  },
  // Retain legacy esteps_* aliases for backwards compatibility
  {
    name: "esteps_probe",
    description: "Alias for cranklink_probe.",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "esteps_get_schema",
    description: "Alias for cranklink_get_schema.",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "esteps_read_settings",
    description: "Alias for cranklink_read_settings.",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "esteps_set_setting",
    description: "Alias for cranklink_set_setting.",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string" },
        value: { type: "string" },
      },
      required: ["name", "value"],
    },
  },
  {
    name: "esteps_batch_settings",
    description: "Alias for cranklink_batch_settings.",
    inputSchema: {
      type: "object",
      properties: { operations: { type: "array" } },
      required: ["operations"],
    },
  },
  {
    name: "esteps_create_backup",
    description: "Alias for cranklink_create_backup.",
    inputSchema: { type: "object", properties: {} },
  },
];

export class WebMCPServer {
  private controller: SessionController;

  constructor(controller: SessionController) {
    this.controller = controller;
    if (typeof window !== "undefined") {
      this.installBridge();
    }
  }

  private installBridge(): void {
    window.addEventListener("message", async (event) => {
      // Only this page and same-origin windows may drive the device; another
      // origin (a framing or opening page) must not read serials or settings.
      const origin = window.location.origin;
      if (event.source !== window && event.origin !== origin) return;
      const data = event.data;
      if (data && data.jsonrpc === "2.0" && data.method) {
        try {
          const result = await this.handleRequest(data.method, data.params);
          event.source?.postMessage(
            { jsonrpc: "2.0", id: data.id, result },
            // @ts-ignore
            origin,
          );
        } catch (err: any) {
          event.source?.postMessage(
            {
              jsonrpc: "2.0",
              id: data.id,
              error: { code: -32603, message: err?.message || String(err) },
            },
            // @ts-ignore
            origin,
          );
        }
      }
    });
  }

  public async handleRequest(method: string, params: any = {}): Promise<any> {
    const state = this.controller.getState();

    switch (method) {
      case "tools/list":
        return { tools: MCP_TOOLS };

      case "resources/list":
        return {
          resources: [
            {
              uri: "cranklink://device/identity",
              name: "Component Identity",
              mimeType: "application/json",
            },
            {
              uri: "cranklink://device/settings",
              name: "Live Component Settings",
              mimeType: "application/json",
            },
            {
              uri: "esteps://device/identity",
              name: "Drive Unit Identity (Legacy Alias)",
              mimeType: "application/json",
            },
            {
              uri: "esteps://device/settings",
              name: "Live Drive Unit Settings (Legacy Alias)",
              mimeType: "application/json",
            },
          ],
        };

      case "resources/read":
        if (params.uri === "cranklink://device/identity" || params.uri === "esteps://device/identity") {
          return {
            contents: [
              {
                uri: params.uri,
                mimeType: "application/json",
                text: JSON.stringify(state.identity, null, 2),
              },
            ],
          };
        }
        if (params.uri === "cranklink://device/settings" || params.uri === "esteps://device/settings") {
          const flat = Object.fromEntries(
            Object.entries(state.liveValues).map(([k, v]) => [k, v.value]),
          );
          return {
            contents: [
              {
                uri: params.uri,
                mimeType: "application/json",
                text: JSON.stringify(flat, null, 2),
              },
            ],
          };
        }
        throw new Error(`Unknown MCP resource: ${params.uri}`);

      case "tools/call":
        return this.callTool(params.name, params.arguments || {});

      default:
        throw new Error(`Unsupported MCP method: ${method}`);
    }
  }

  /** Device tools must not interleave with a flash, a save or a dead link. */
  private requireConnectedIdle(): void {
    const { phase, linkLost } = this.controller.getState();
    if (phase !== "ready" || linkLost) {
      throw new Error("Wait for a connected, idle component before using device tools.");
    }
  }

  private async callTool(name: string, args: any): Promise<any> {
    const state = this.controller.getState();

    if (name === "cranklink_list_components") {
      return args.refresh ? this.controller.refreshUnits() : state.units;
    }
    if (name === "cranklink_select_component") {
      if (!Number.isInteger(args.slot)) throw new Error("Select a component using an inventory slot.");
      return this.controller.selectComponent(args.slot);
    }

    if (name === "cranklink_probe" || name === "esteps_probe") {
      if (!state.identity) throw new Error("Select a connected component first.");
      return this.controller.getState().identity;
    }

    if (name === "cranklink_get_schema" || name === "esteps_get_schema") {
      return this.controller.getState().schema;
    }

    if (name === "cranklink_read_settings" || name === "esteps_read_settings") {
      this.requireConnectedIdle();
      const entries = await this.controller.readAllParameters();
      return Object.fromEntries(Object.entries(entries).map(([k, v]) => [k, v.value]));
    }

    if (name === "cranklink_create_backup" || name === "esteps_create_backup") {
      this.requireConnectedIdle();
      const filename = await this.controller.captureManualSnapshot();
      return { filename };
    }

    if (
      name === "cranklink_set_setting" ||
      name === "esteps_set_setting" ||
      name === "cranklink_batch_settings" ||
      name === "esteps_batch_settings"
    ) {
      const ops: Array<{ name: string; value: string | number }> =
        name === "cranklink_set_setting" || name === "esteps_set_setting"
          ? [{ name: args.name, value: args.value }]
          : (args.operations || [])
              .filter((op: any) => op.op === "set")
              .map((op: any) => ({ name: op.name, value: op.value }));

      const requestedNames = new Set<string>();
      for (const op of ops) {
        if (requestedNames.has(op.name)) throw new Error("Request each setting only once in a change review.");
        requestedNames.add(op.name);
      }

      while (
        this.controller.getState().phase === "connecting" ||
        this.controller.getState().phase === "probing" ||
        this.controller.getState().phase === "reading"
      ) {
        await new Promise((r) => setTimeout(r, 50));
      }
      this.requireConnectedIdle();

      if (Object.keys(this.controller.getState().stagedValues).length) {
        throw new Error("Save or discard staged changes before requesting an agent write.");
      }
      if (this.controller.getState().mcpApproval) {
        throw new Error("Finish the pending change review before requesting another agent write.");
      }

      let currentLive = this.controller.getState().liveValues;
      if (
        Object.keys(currentLive).length === 0 &&
        this.controller.getState().transport &&
        this.controller.getState().phase === "ready"
      ) {
        currentLive = await this.controller.readAllParameters();
      }

      const diff = ops.map((op) => ({
        name: op.name,
        before: currentLive[op.name]?.value ?? null,
        after: op.value,
      }));

      const target = this.controller.getState().identity;
      if (!target) throw new Error("Select a connected component first.");
      for (const op of ops) {
        const spec = this.controller.getState().schema.find((setting) => setting.name === op.name);
        if (!spec || spec.read_only || !currentLive[op.name]) throw new Error(`Setting ${op.name} is unavailable or read-only.`);
      }

      const approved = await this.controller.requestMcpApproval(name, args, diff);
      if (!approved) {
        throw new Error("Transaction rejected by operator.");
      }

      if (Object.keys(this.controller.getState().stagedValues).length) {
        throw new Error("Staged changes appeared during review. Save or discard them and review the agent changes again.");
      }

      const currentTarget = this.controller.getState().identity;
      if (currentTarget?.serial !== target.serial || currentTarget?.modelSlug !== target.modelSlug || currentTarget?.slot !== target.slot) {
        throw new Error("The selected component changed during review. Review the changes again.");
      }

      try {
        for (const op of ops) this.controller.stageParameter(op.name, op.value);
        const staged = this.controller.getState().stagedValues;
        const live = this.controller.getState().liveValues;
        const requestedValue = (op: { name: string; value: string | number }) => {
          const spec = this.controller.getState().schema.find((setting) => setting.name === op.name);
          return spec?.value_type === "number" || spec?.value_type === "enum" ? Number(op.value) : op.value;
        };
        const changedPlan = Object.entries(staged).some(([setting, value]) => {
          const reviewed = ops.find((op) => op.name === setting);
          return !reviewed || String(value) !== String(requestedValue(reviewed));
        }) || ops.some((op) => String(staged[op.name] ?? live[op.name]?.value) !== String(requestedValue(op)));
        if (changedPlan) {
          throw new Error("The staged plan differs from the approved changes. Include the required destination and speed settings explicitly in a new review.");
        }
      } catch (error) {
        // No operator edits existed when this synchronous staging pass began.
        // Clear only this agent's temporary plan; failed backend writes retain it.
        this.controller.discardAllStaged();
        throw error;
      }
      return this.controller.commitStagedBatch();
    }

    throw new Error(`Unknown MCP tool: ${name}`);
  }
}
