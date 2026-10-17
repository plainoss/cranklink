/**
 * main.ts - Bootstrap for cranklink Field Instrument (`web-instrument/`).
 */

import { createRoot } from "react-dom/client";
import React from "react";
import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import "../app.js";
import { SessionController } from "./core/sessionController.ts";
import { WebMCPServer } from "./core/webMcpServer.ts";
import { App } from "./App.tsx";

const controller = new SessionController();
const mcpServer = new WebMCPServer(controller);

const rootRoute = createRootRoute({
  component: () => React.createElement(App, { controller }),
});
const appRoutes = [
  createRoute({ getParentRoute: () => rootRoute, path: "/" }),
  createRoute({ getParentRoute: () => rootRoute, path: "connect" }),
  createRoute({ getParentRoute: () => rootRoute, path: "overview" }),
  createRoute({ getParentRoute: () => rootRoute, path: "tune/$category" }),
  createRoute({ getParentRoute: () => rootRoute, path: "firmware" }),
  createRoute({ getParentRoute: () => rootRoute, path: "backup" }),
  createRoute({ getParentRoute: () => rootRoute, path: "diagnostics" }),
];
const router = createRouter({
  routeTree: rootRoute.addChildren(appRoutes),
  history: createHashHistory(),
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

(window as any).estepsInstrumentController = controller;
(window as any).cranklinkInstrumentController = controller;
(window as any).webMcpServer = mcpServer;
(window as any).mcp = mcpServer;

const mount = document.getElementById("instrument-root") || document.getElementById("app");
if (mount) {
  const root = createRoot(mount);
  root.render(React.createElement(RouterProvider, { router }));
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
