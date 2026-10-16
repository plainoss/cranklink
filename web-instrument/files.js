export class BrowserFiles {
  constructor() {
    this.files = new Map();
  }
  async initialize() {
    if (!globalThis.indexedDB)
      throw new Error(
        "IndexedDB is required for durable firmware journals and settings backups",
      );
    this.db ??= await new Promise((resolve, reject) => {
      const r = indexedDB.open("esteps-files-v1", 1);
      r.onupgradeneeded = () =>
        r.result.createObjectStore("files", { keyPath: "path" });
      r.onsuccess = () => resolve(r.result);
      r.onerror = () => reject(r.error);
    });
    const entries = await new Promise((resolve, reject) => {
      const t = this.db.transaction("files", "readonly");
      const r = t.objectStore("files").getAll();
      r.onsuccess = () => resolve(r.result);
      r.onerror = () => reject(r.error);
    });
    for (const entry of entries) {
      const bytes = new Uint8Array(entry.bytes);
      this.files.set(entry.path, {
        chunks: [bytes],
        size: bytes.length,
        dirty: false,
        revision: 0,
      });
    }
  }
  name(path) {
    if (typeof path !== "string" || !path || path.includes("\0"))
      throw new Error("Invalid virtual file name");
    return path;
  }
  bytes(path) {
    const entry = this.files.get(this.name(path));
    if (!entry) throw new Error(`File not found: ${path}`);
    const out = new Uint8Array(entry.size);
    let offset = 0;
    for (const chunk of entry.chunks) {
      out.set(chunk, offset);
      offset += chunk.length;
    }
    return out;
  }
  async persist(path, bytes, exclusive = false) {
    try {
      await new Promise((resolve, reject) => {
        const t = this.db.transaction("files", "readwrite", {
          durability: "strict",
        });
        const store = t.objectStore("files");
        const record = { path, bytes };
        if (exclusive) store.add(record);
        else store.put(record);
        t.oncomplete = resolve;
        t.onerror = () => reject(t.error);
        t.onabort = () =>
          reject(t.error || new Error("File transaction aborted"));
      });
    } catch (error) {
      if (error?.name === "ConstraintError")
        throw new Error(`File already exists: ${path}`);
      throw error;
    }
  }
  async sync(path) {
    const entry = this.files.get(this.name(path));
    if (!entry) throw new Error(`File not found: ${path}`);
    // Serialize snapshots, retaining any bytes appended while IndexedDB commits.
    const pending = (entry.syncing || Promise.resolve())
      .catch(() => {})
      .then(async () => {
        if (!entry.dirty) return;
        const revision = entry.revision,
          bytes = this.bytes(path);
        await this.persist(path, bytes);
        if (entry.revision === revision) {
          entry.chunks = [bytes];
          entry.dirty = false;
        }
      });
    entry.syncing = pending;
    await pending;
  }
  async import(path, bytes) {
    path = this.name(path);
    if (this.files.has(path)) throw new Error(`File already exists: ${path}`);
    bytes = Uint8Array.from(bytes);
    await this.persist(path, bytes, true);
    this.files.set(path, {
      chunks: [bytes],
      size: bytes.length,
      dirty: false,
      revision: 0,
    });
  }
  async request(op, p) {
    const path = this.name(p.path);
    if (op === "fs.read") {
      const entry = this.files.get(path);
      if (!entry) throw new Error(`File not found: ${path}`);
      if (p.max_bytes !== undefined && entry.size > p.max_bytes)
        throw new Error("File exceeds size limit");
      return this.bytes(path);
    }
    if (op === "fs.create") {
      const existing = this.files.get(path);
      if (existing && p.create_new)
        throw new Error(`File already exists: ${path}`);
      if (!existing && !p.create && !p.create_new)
        throw new Error(`File not found: ${path}`);
      if (!existing || p.truncate) {
        await this.persist(path, new Uint8Array(), !existing || p.create_new);
        this.files.set(path, {
          chunks: [],
          size: 0,
          dirty: false,
          revision: 0,
        });
      }
      return new Uint8Array();
    }
    if (op === "fs.append") {
      const entry = this.files.get(path);
      if (!entry) throw new Error(`File not found: ${path}`);
      const bytes = Uint8Array.from(p.data);
      if (entry.size + bytes.length > 64 * 1024 * 1024)
        throw new Error("Virtual file exceeds 64 MiB limit");
      entry.chunks.push(bytes);
      entry.size += bytes.length;
      entry.dirty = true;
      ++entry.revision;
      return new Uint8Array();
    }
    if (op === "fs.sync") {
      await this.sync(path);
      return new Uint8Array();
    }
    throw new Error(`Unsupported file operation ${op}`);
  }
  async syncAll() {
    for (const path of this.files.keys()) await this.sync(path);
  }
  list() {
    return [...this.files].map(([path, e]) => ({ path, bytes: e.size }));
  }
}
