let control,
  buffer,
  sequence = 0,
  poisoned = false,
  simulatorFlag = null;
const sleeper = new Int32Array(new SharedArrayBuffer(4));
export function configure(shared) {
  control = new Int32Array(shared.control);
  buffer = new Uint8Array(shared.buffer);
  if (shared.simulator) {
    simulatorFlag = new Int32Array(shared.simulator);
  }
}
// Rust imports host_call with `catch` and reads string errors via as_string();
// throwing Error objects would surface as JsValue debug text with a JS stack.
export function host_call(op, payload, timeout) {
  if (poisoned)
    throw "Browser host failed; reload before another command";
  if (!control) throw "Worker host is not initialized";
  const id = ++sequence;
  Atomics.store(control, 3, id);
  Atomics.store(control, 0, 0);
  const bytes = Uint8Array.from(payload);
  postMessage({ type: "rpc", id, op, payload: bytes, timeout }, [bytes.buffer]);
  const deadline = performance.now() + timeout + 5000;
  for (;;) {
    const state = Atomics.load(control, 0);
    if (state === 1) break;
    const remaining = deadline - performance.now();
    if (remaining <= 0) {
      // Poison this worker permanently. It can never reuse a buffer being published.
      poisoned = true;
      Atomics.compareExchange(control, 0, 0, 2);
      postMessage({ type: "host-failed", op });
      throw `Browser host timed out: ${op}; outcome may be uncertain. Reload before another command.`;
    }
    Atomics.wait(control, 0, state, remaining);
  }
  const length = Atomics.load(control, 2);
  const response = buffer.slice(0, length);
  if (Atomics.load(control, 1) !== 0)
    throw new TextDecoder().decode(response);
  return response;
}
export function host_emit(stream, text) {
  postMessage({ type: "output", stream, text });
}
export function host_now_ms() {
  return performance.timeOrigin + performance.now();
}
export function host_sleep(ms) {
  if (simulatorFlag && Atomics.load(simulatorFlag, 0) === 1) {
    return;
  }
  Atomics.wait(sleeper, 0, 0, ms);
}
