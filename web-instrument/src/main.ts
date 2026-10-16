/**
 * main.ts - Bootstrap for esteps Field Instrument (`web-instrument/`).
 */

import "../app.js";
import { SessionController } from "./core/sessionController.ts";
import { WebMCPServer } from "./core/webMcpServer.ts";
import "./ui/instrumentApp.ts";
import type { EstepsInstrumentElement } from "./ui/instrumentApp.ts";

const controller = new SessionController();
const mcpServer = new WebMCPServer(controller);

(window as any).estepsInstrumentController = controller;
(window as any).webMcpServer = mcpServer;
(window as any).mcp = mcpServer;

const mount = document.getElementById("instrument-root") as EstepsInstrumentElement | null;
if (mount) {
  customElements.upgrade(mount);
  mount.bindController(controller);
}

// Wait for WASM worker host (`window.estepsTerminal`) to initialize and attach
function bindHostWhenReady() {
  const term = (window as any).estepsTerminal;
  if (term && term.status().ready) {
    controller.attachTerminal(term);
    return;
  }
  const timer = setInterval(() => {
    const t = (window as any).estepsTerminal;
    if (t && t.status().ready) {
      clearInterval(timer);
      controller.attachTerminal(t);
    }
  }, 40);
}

bindHostWhenReady();
