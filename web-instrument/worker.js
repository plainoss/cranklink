import { configure } from "./host.js";
import init, { run, disconnect_ble } from "./pkg/cranklink.js";
let initialized = false;
self.onmessage = async (event) => {
  const message = event.data;
  if (message.type === "init") {
    try {
      configure(message.shared);
      await init();
      initialized = true;
      postMessage({ type: "ready" });
    } catch (error) {
      postMessage({ type: "fatal", error: String(error) });
    }
    return;
  }
  if (message.type === "run" && initialized) {
    try {
      const code = run(JSON.stringify(message.args));
      postMessage({ type: "done", code });
    } catch (error) {
      postMessage({
        type: "fatal",
        error: `WASM trapped: ${error}. An active device operation may be uncertain; preserve the journal.`,
      });
    }
  }
  if (message.type === "ble-disconnect" && initialized) {
    try {
      postMessage({ type: "ble-disconnected", code: disconnect_ble() });
    } catch (error) {
      // A trap does not unwind; the instance's state is undefined from here on.
      postMessage({
        type: "ble-disconnected",
        code: 1,
        trapped: true,
        error: `WASM trapped during Bluetooth cleanup: ${error}. Reload before another command and check the bike.`,
      });
    }
  }
};
