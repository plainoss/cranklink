//! The browser runs the same synchronous CLI in a dedicated worker. Host RPC
//! bridges WebUSB and durable browser files; no firmware logic lives in JS.
use wasm_bindgen::prelude::*;
#[wasm_bindgen(raw_module = "/host.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    pub fn host_call(op: &str, payload: &[u8], timeout_ms: u32) -> Result<Vec<u8>, JsValue>;
    pub fn host_emit(stream: &str, text: &str);
    pub fn host_now_ms() -> f64;
    pub fn host_sleep(milliseconds: u32);
}
pub fn rpc(op: &str, payload: &[u8], timeout_ms: u32) -> std::io::Result<Vec<u8>> {
    host_call(op, payload, timeout_ms).map_err(|e| {
        std::io::Error::other(
            e.as_string()
                .unwrap_or_else(|| format!("browser host operation {op} failed: {e:?}")),
        )
    })
}
#[wasm_bindgen]
pub fn run(args_json: &str) -> i32 {
    let result = serde_json::from_str::<Vec<String>>(args_json)
        .map_err(anyhow::Error::from)
        .and_then(crate::cli::execute);
    match result {
        Ok(()) => 0,
        Err(error) => {
            host_emit("stderr", &format!("error: {error:#}\n"));
            1
        }
    }
}
