/**
 * main.ts - Bootstrap for cranklink Field Instrument (`web-instrument/`).
 */

import { createRoot } from "react-dom/client";
import React from "react";
import "../app.js";
import { SessionController } from "./core/sessionController.ts";
import { WebMCPServer } from "./core/webMcpServer.ts";
import { App } from "./App.tsx";

const controller = new SessionController();
const mcpServer = new WebMCPServer(controller);

(window as any).estepsInstrumentController = controller;
(window as any).cranklinkInstrumentController = controller;
(window as any).webMcpServer = mcpServer;
(window as any).mcp = mcpServer;

const mount = document.getElementById("instrument-root") || document.getElementById("app");
if (mount) {
  const root = createRoot(mount);
  root.render(React.createElement(App, { controller }));
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
