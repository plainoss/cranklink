import { it } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import net from "node:net";
import os from "node:os";

// A loopback server on a free port, started from `cwd`.
async function listenFrom(cwd: string) {
  const listen = "require('net').createServer().listen(0, '127.0.0.1', function () { console.log(this.address().port) })";
  const server = spawn(process.execPath, ["-e", listen], { cwd, stdio: ["ignore", "pipe", "inherit"] });
  const [line] = await once(server.stdout!, "data");
  return { server, port: Number(String(line).trim()) };
}

function ensurePortFree(port: number) {
  return spawnSync(process.execPath, ["scripts/ensure-port-free.mjs", String(port)], {
    cwd: import.meta.dirname,
    encoding: "utf8",
    env: { ...process.env, CRANKLINK_E2E_REUSE: undefined },
  });
}

it("ensure-port-free refuses a server from another directory and leaves it running", async () => {
  const { server, port } = await listenFrom(os.tmpdir());
  try {
    const result = ensurePortFree(port);
    assert.equal(result.status, 1, result.stdout);
    assert.match(result.stderr, /CRANKLINK_E2E_PORT/);
    const client = net.connect(port, "127.0.0.1");
    await once(client, "connect");
    client.destroy();
  } finally {
    server.kill();
  }
});

it("ensure-port-free stops a server left from web-instrument", { skip: spawnSync("lsof", ["-v"]).error ? "needs lsof" : false }, async () => {
  const { server, port } = await listenFrom(import.meta.dirname);
  const exited = once(server, "exit");
  try {
    const result = ensurePortFree(port);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(await exited, [null, "SIGTERM"]);
  } finally {
    server.kill();
  }
});
