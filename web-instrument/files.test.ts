import { it } from "node:test";
import assert from "node:assert/strict";
import { BrowserFiles } from "./files.js";

// An in-memory IndexedDB stand-in whose writes can be made to abort.
function fakeDb() {
  const records = new Map<string, { path: string; bytes: Uint8Array }>();
  const state = { failWrites: false };
  const db = {
    transaction() {
      const t: Record<string, any> = { error: null };
      const write = (record: { path: string; bytes: Uint8Array }) =>
        queueMicrotask(() => {
          if (state.failWrites) {
            t.error = new DOMException("Quota exceeded", "QuotaExceededError");
            t.onabort?.();
            return;
          }
          records.set(record.path, { path: record.path, bytes: record.bytes.slice() });
          t.oncomplete?.();
        });
      t.objectStore = () => ({
        getAll() {
          const r: Record<string, any> = {};
          queueMicrotask(() => {
            r.result = [...records.values()].map((e) => ({ path: e.path, bytes: e.bytes.slice() }));
            r.onsuccess?.();
          });
          return r;
        },
        put: write,
        add: write,
      });
      return t;
    },
  };
  return { db, state };
}

it("initialize keeps journal bytes whose sync failed", async () => {
  (globalThis as any).indexedDB ??= {};
  const { db, state } = fakeDb();
  // Preset the store so initialize() skips indexedDB.open.
  const files = Object.assign(new BrowserFiles(), { db });
  await files.initialize();
  const append = (text: string) =>
    files.request("fs.append", { path: "j.jsonl", data: [...new TextEncoder().encode(text)] });
  await files.request("fs.create", { path: "j.jsonl", create_new: true });
  await append("line-1\n");
  await files.request("fs.sync", { path: "j.jsonl" });

  state.failWrites = true;
  await append("line-2\n");
  await assert.rejects(files.syncAll(), { name: "QuotaExceededError" });
  // The next command reloads the store before it runs.
  await files.initialize();
  assert.equal(new TextDecoder().decode(files.bytes("j.jsonl")), "line-1\nline-2\n");

  state.failWrites = false;
  await files.syncAll();
  const reloaded = Object.assign(new BrowserFiles(), { db });
  await reloaded.initialize();
  assert.equal(new TextDecoder().decode(reloaded.bytes("j.jsonl")), "line-1\nline-2\n");
});
