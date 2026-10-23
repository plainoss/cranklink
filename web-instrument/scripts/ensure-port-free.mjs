import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { execFileSync } from "node:child_process";

// Frees the e2e port of a server started from this web-instrument directory.
// A server from anywhere else, such as another worktree's run, is left alone.
const port = Number(process.argv[2] || process.env.CRANKLINK_E2E_PORT) || 8030;
const root = fs.realpathSync(path.resolve(import.meta.dirname, ".."));
const reuse = !process.env.CI && process.env.CRANKLINK_E2E_REUSE === "1";

function checkPort(targetPort) {
  return new Promise((resolve) => {
    const tester = net.createServer()
      .once("error", (err) => {
        if (err.code === "EADDRINUSE") {
          resolve(true); // in use
        } else {
          resolve(false);
        }
      })
      .once("listening", () => {
        tester.close(() => resolve(false)); // free
      })
      .listen(targetPort, "127.0.0.1");
  });
}

function listeningPids() {
  try {
    // Only the listening server: `lsof -ti :PORT` also lists processes that are
    // merely connected to it, such as a browser or another run's Chromium.
    const pids = execFileSync("lsof", ["-ti", `tcp:${port}`, "-sTCP:LISTEN"], { encoding: "utf8" });
    return [...new Set(pids.split(/\s+/).filter(Boolean))];
  } catch {
    // lsof exited with non-zero (no process found or not available)
    return [];
  }
}

function cwdOf(pid) {
  try {
    return fs.realpathSync(`/proc/${pid}/cwd`);
  } catch {
    // Not Linux, or a process we may not inspect.
  }
  try {
    const fields = execFileSync("lsof", ["-a", "-p", pid, "-d", "cwd", "-Fn"], { encoding: "utf8" });
    const name = fields.split("\n").find((line) => line.startsWith("n"));
    return name ? fs.realpathSync(name.slice(1)) : null;
  } catch {
    return null;
  }
}

function refuse(reason) {
  console.error(`[pretest] ${reason}. Stop it or rerun with CRANKLINK_E2E_PORT=<free port>.`);
  process.exit(1);
}

if (await checkPort(port)) {
  const pids = listeningPids();
  if (pids.length === 0) refuse(`Port ${port} is in use by a process that could not be identified`);
  for (const pid of pids) {
    const cwd = cwdOf(pid);
    if (cwd !== root) refuse(`Port ${port} is served by PID ${pid} from ${cwd ?? "an unknown directory"}`);
  }
  if (reuse) {
    console.log(`[pretest] Reusing the server on port ${port} from this directory.`);
    process.exit(0);
  }
  console.log(`[pretest] Port ${port} is held by a server from this directory. Terminating it...`);
  for (const pid of pids) {
    try {
      process.kill(Number(pid), "SIGTERM");
    } catch {
      // Process might already have exited
    }
  }
  // Wait up to 2 seconds for port to clear
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 100));
    if (!(await checkPort(port))) {
      console.log(`[pretest] Port ${port} is now free.`);
      process.exit(0);
    }
  }
  refuse(`Port ${port} is still in use after stopping PID ${pids.join(", ")}`);
}
