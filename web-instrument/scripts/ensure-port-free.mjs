import net from "node:net";
import { execSync } from "node:child_process";

const port = Number(process.argv[2]) || 8030;

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

const inUse = await checkPort(port);
if (inUse) {
  console.log(`[pretest] Port ${port} is currently occupied. Terminating stale process...`);
  try {
    const pids = execSync(`lsof -ti :${port}`, { encoding: "utf8" }).trim();
    if (pids) {
      for (const pid of pids.split(/\s+/)) {
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
    }
  } catch {
    // lsof exited with non-zero (no process found or not available)
  }
  console.warn(`[pretest] Warning: Port ${port} may still be in use. Tests may conflict.`);
}
