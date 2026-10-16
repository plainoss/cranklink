/**
 * webMcpServer.ts - In-Browser Model Context Protocol (MCP) Server for esteps Field Instrument.
 * Exposes structured tools and resources over JSON-RPC 2.0 (window.postMessage and direct API)
 * with human-in-the-loop confirmation for any write transaction.
 */

import type { SessionController } from "./sessionController.ts";

export const MCP_TOOLS = [
  {
    name: "cranklink_probe",
    description: "Read e-bike drive unit identity, serial number, and dual MCU firmware versions.",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
  {
    name: "cranklink_get_schema",
    description: "Return the active drive unit setting schema with valid ranges, units, and categories.",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
  {
    name: "cranklink_read_settings",
    description: "Read all live parameter values from the connected drive unit.",
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
    description: "Stage and apply multiple setting changes in a single atomic session with human confirmation.",
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
    description: "Export an immediate, durable JSON snapshot of all live drive unit parameters.",
    inputSchema: {
      type: "object",
      properties: {},
    },
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
      const data = event.data;
      if (data && data.jsonrpc === "2.0" && data.method) {
        try {
          const result = await this.handleRequest(data.method, data.params);
          event.source?.postMessage(
            { jsonrpc: "2.0", id: data.id, result },
            // @ts-ignore
            event.origin || "*",
          );
        } catch (err: any) {
          event.source?.postMessage(
            {
              jsonrpc: "2.0",
              id: data.id,
              error: { code: -32603, message: err?.message || String(err) },
            },
            // @ts-ignore
            event.origin || "*",
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
              name: "Drive Unit Identity",
              mimeType: "application/json",
            },
            {
              uri: "cranklink://device/settings",
              name: "Live Drive Unit Settings",
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

  private async callTool(name: string, args: any): Promise<any> {
    const state = this.controller.getState();

    if (name === "cranklink_probe" || name === "esteps_probe") {
      if (!state.identity && state.transport) {
        await this.controller.connectAndSync(state.transport);
      }
      return this.controller.getState().identity;
    }

    if (name === "cranklink_get_schema" || name === "esteps_get_schema") {
      return this.controller.getState().schema;
    }

    if (name === "cranklink_read_settings" || name === "esteps_read_settings") {
      const entries = await this.controller.readAllParameters();
      return Object.fromEntries(Object.entries(entries).map(([k, v]) => [k, v.value]));
    }

    if (name === "cranklink_create_backup" || name === "esteps_create_backup") {
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

      const diff = ops.map((op) => ({
        name: op.name,
        before: state.liveValues[op.name]?.value ?? null,
        after: op.value,
      }));

      const approved = await this.controller.requestMcpApproval(name, args, diff);
      if (!approved) {
        throw new Error("Transaction rejected by operator.");
      }

      for (const op of ops) {
        this.controller.stageParameter(op.name, op.value);
      }
      return this.controller.commitStagedBatch();
    }

    throw new Error(`Unknown MCP tool: ${name}`);
  }
}
