import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { execFileSync } from "node:child_process";

function buildRevision(): string {
  try {
    const revision = execFileSync("git", ["rev-parse", "--short=12", "HEAD"], { encoding: "utf8" }).trim();
    const dirty = execFileSync("git", ["status", "--porcelain", "--untracked-files=no"], { encoding: "utf8" }).trim();
    return revision + (dirty ? "-dirty" : "");
  } catch {
    return "unknown";
  }
}

export default defineConfig({
  base: "./",
  define: { __CRANKLINK_BUILD__: JSON.stringify(buildRevision()) },
  plugins: [
    react(),
    tailwindcss(),
  ],
  server: {
    port: 8030,
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
  preview: {
    port: 8030,
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
  build: {
    target: "es2022",
    sourcemap: false,
  },
});
