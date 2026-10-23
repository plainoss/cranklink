//! Persistent JSON-RPC 2.0 session driver for autonomous agents, CI runners, and scripts.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    /// Echoes the request id; parse errors carry `Some(Value::Null)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub name: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl JsonRpcError {
    pub fn not_connected(hint: Option<&str>) -> Self {
        Self {
            code: 4001,
            name: "NOT_CONNECTED".into(),
            message: "No active adapter session. Run session.connect first.".into(),
            data: None,
            hint: hint.map(str::to_string),
        }
    }

    pub fn invalid_params(msg: impl Into<String>, hint: Option<&str>) -> Self {
        Self {
            code: -32602,
            name: "INVALID_PARAMS".into(),
            message: msg.into(),
            data: None,
            hint: hint.map(str::to_string),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            name: "METHOD_NOT_FOUND".into(),
            message: format!("Unknown method '{method}'"),
            data: None,
            hint: Some("Supported: session.connect, session.disconnect, session.status, probe, units, settings.schema, firmware.inspect, exit".into()),
        }
    }

    pub fn hardware_error(err: anyhow::Error) -> Self {
        let text = format!("{err:#}");
        let (name, code, hint) = if text.contains("serial mismatch") {
            (
                "SERIAL_MISMATCH",
                4002,
                Some("Verify the motor serial number and bus slot before connecting."),
            )
        } else if text.contains("authentication") || text.contains("challenge") {
            (
                "AUTH_FAILED",
                4003,
                Some(
                    "Drive-unit authentication failed or is unsupported on this firmware version.",
                ),
            )
        } else if text.contains("range") || text.contains("bounds") {
            (
                "VALUE_OUT_OF_RANGE",
                4004,
                Some("Check settings.schema for allowed ranges and units."),
            )
        } else if text.contains("timeout") {
            (
                "TIMEOUT",
                4006,
                Some("Device did not respond. Check cable connections and motor power."),
            )
        } else {
            ("HARDWARE_ERROR", 4000, None)
        };

        Self {
            code,
            name: name.into(),
            message: text,
            data: None,
            hint: hint.map(str::to_string),
        }
    }
}

/// Absent or null selectors choose automatically; anything else must be a
/// bus slot or a component name, never silently ignored.
fn probe_target(params: &Value) -> std::result::Result<(Option<u8>, Option<String>), String> {
    let slot = match params.get("slot") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_u64()
                .filter(|slot| *slot < 32)
                .map(|slot| slot as u8)
                .ok_or_else(|| format!("slot must be an integer 0..31, got {value}"))?,
        ),
    };
    let component = match params.get("component") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name.clone()),
        Some(value) => return Err(format!("component must be a model name, got {value}")),
    };
    Ok((slot, component))
}

pub fn run_session(
    default_port: Option<String>,
    trace_path: Option<PathBuf>,
    expect_serial: Option<String>,
) -> Result<()> {
    run_session_io(
        std::io::stdin().lock(),
        std::io::stdout(),
        default_port,
        trace_path,
        expect_serial,
    )
}

fn run_session_io(
    mut reader: impl BufRead,
    mut stdout: impl Write,
    default_port: Option<String>,
    trace_path: Option<PathBuf>,
    expect_serial: Option<String>,
) -> Result<()> {
    let mut line = Vec::new();

    let mut session: Option<crate::device::Session> = None;
    let mut active_port = default_port.unwrap_or_else(|| "auto".to_string());
    let mut active_serial = expect_serial;
    let mut active_identity: Option<crate::device::Identity> = None;

    // Emit initial ready notification
    let ready_notice = json!({
        "jsonrpc": "2.0",
        "method": "event.ready",
        "params": {
            "version": env!("CARGO_PKG_VERSION"),
            "default_port": active_port,
        }
    });
    writeln!(stdout, "{}", serde_json::to_string(&ready_notice)?)?;
    stdout.flush()?;

    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break; // EOF
        }

        // Invalid UTF-8 is a parse error for this line, not a fatal one; never
        // repair it, which would silently change ports, paths or names.
        let parsed = std::str::from_utf8(&line)
            .map_err(|e| format!("invalid UTF-8: {e}"))
            .and_then(|text| {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return Ok(None);
                }
                serde_json::from_str::<JsonRpcRequest>(trimmed)
                    .map(Some)
                    .map_err(|e| format!("Malformed JSON: {e}"))
            });
        let req = match parsed {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(message) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: Some(Value::Null),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        name: "PARSE_ERROR".into(),
                        message,
                        data: None,
                        hint: Some("Send valid JSON-RPC 2.0 lines terminated by newline.".into()),
                    }),
                };
                writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let id = req.id.clone();
        let method = req.method.as_str();

        let resp = match method {
            "exit" | "quit" => {
                if let Some(mut s) = session.take() {
                    let _ = s.normal_stop();
                }
                let ok_resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({ "exited": true })),
                    error: None,
                };
                writeln!(stdout, "{}", serde_json::to_string(&ok_resp)?)?;
                stdout.flush()?;
                break;
            }
            "session.connect" | "connect" => {
                if let Some(ser) = req.params.get("expect_serial").and_then(Value::as_str) {
                    match super::sanitize_serial(ser) {
                        Ok(serial) => active_serial = Some(serial),
                        Err(e) => {
                            let err_resp = JsonRpcResponse {
                                jsonrpc: "2.0",
                                id,
                                result: None,
                                error: Some(JsonRpcError::invalid_params(format!("{e:#}"), None)),
                            };
                            writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                            stdout.flush()?;
                            continue;
                        }
                    }
                }
                if let Some(port_param) = req.params.get("port").and_then(Value::as_str) {
                    active_port = port_param.to_string();
                }

                // If existing session, close it cleanly first
                if let Some(mut s) = session.take() {
                    let _ = s.normal_stop();
                }

                match crate::device::Session::open(&active_port, trace_path.as_deref()).and_then(
                    |mut s| {
                        let power = s.initialize()?;
                        Ok((s, power))
                    },
                ) {
                    Ok((s, power)) => {
                        session = Some(s);
                        active_identity = None;
                        JsonRpcResponse {
                            jsonrpc: "2.0",
                            id,
                            result: Some(json!({
                                "connected": true,
                                "port": active_port,
                                "power": power,
                            })),
                            error: None,
                        }
                    }
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(JsonRpcError::hardware_error(e)),
                    },
                }
            }
            "session.disconnect" | "disconnect" => {
                let stopped = if let Some(mut s) = session.take() {
                    s.normal_stop().is_ok()
                } else {
                    false
                };
                active_identity = None;
                JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({ "connected": false, "stopped": stopped })),
                    error: None,
                }
            }
            "session.status" | "status" => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(json!({
                    "connected": session.is_some(),
                    "port": active_port,
                    "expect_serial": active_serial,
                    "identity": active_identity,
                })),
                error: None,
            },
            "probe" => {
                // Validate parameters before any hardware is opened.
                let (slot, comp) = match probe_target(&req.params) {
                    Ok(target) => target,
                    Err(message) => {
                        let err_resp = JsonRpcResponse {
                            jsonrpc: "2.0",
                            id,
                            result: None,
                            error: Some(JsonRpcError::invalid_params(message, None)),
                        };
                        writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                        stdout.flush()?;
                        continue;
                    }
                };
                let comp = comp.as_deref();
                let s = match session.as_mut() {
                    Some(s) => s,
                    None => {
                        // Auto-connect
                        match crate::device::Session::open(&active_port, trace_path.as_deref())
                            .and_then(|mut s| {
                                s.initialize()?;
                                Ok(s)
                            }) {
                            Ok(new_s) => {
                                session = Some(new_s);
                                session.as_mut().unwrap()
                            }
                            Err(e) => {
                                let err_resp = JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id,
                                    result: None,
                                    error: Some(JsonRpcError::hardware_error(e)),
                                };
                                writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                                stdout.flush()?;
                                continue;
                            }
                        }
                    }
                };

                // Probe is read-only: release the PC-link it opened on every path.
                let probed = s.probe_component(slot, comp);
                let stopped = s.normal_stop();
                match super::finish_operation(
                    probed,
                    stopped.context("PC-link Stop acknowledgement"),
                ) {
                    Ok(probe_id) => {
                        if let Some(exp) = &active_serial {
                            if !probe_id.serial.eq_ignore_ascii_case(exp) {
                                let err_resp = JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: 4002,
                                        name: "SERIAL_MISMATCH".into(),
                                        message: format!(
                                            "motor serial mismatch: expected {}, got {}",
                                            exp, probe_id.serial
                                        ),
                                        data: Some(json!({
                                            "expected": exp,
                                            "actual": probe_id.serial,
                                        })),
                                        hint: Some(
                                            "Update expect_serial or check component selection."
                                                .into(),
                                        ),
                                    }),
                                };
                                writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                                stdout.flush()?;
                                continue;
                            }
                        }
                        active_identity = Some(probe_id.identity.clone());
                        JsonRpcResponse {
                            jsonrpc: "2.0",
                            id,
                            result: Some(serde_json::to_value(&probe_id).unwrap_or_default()),
                            error: None,
                        }
                    }
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(JsonRpcError::hardware_error(e)),
                    },
                }
            }
            "units" => match session.as_mut() {
                Some(s) => match s.units() {
                    Ok(units) => JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: Some(json!({ "units": units })),
                        error: None,
                    },
                    Err(e) => JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(JsonRpcError::hardware_error(e)),
                    },
                },
                None => JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: None,
                    error: Some(JsonRpcError::not_connected(Some(
                        "Run session.connect first to open the bus.",
                    ))),
                },
            },
            "settings.schema" => {
                let model_str = req
                    .params
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("ep800-crg");
                match super::settings_model_name(model_str) {
                    Err(error) => JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(JsonRpcError::invalid_params(
                            error.to_string(),
                            Some("Use a motor family alias or catalogued component model."),
                        )),
                    },
                    Ok(model) => {
                        let specs = crate::settings::schema_for_model(model);
                        if let Some(target) = req.params.get("name").and_then(Value::as_str) {
                            if let Some(spec) =
                                specs.iter().find(|s| s.name.eq_ignore_ascii_case(target))
                            {
                                JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id,
                                    result: Some(serde_json::to_value(spec).unwrap_or_default()),
                                    error: None,
                                }
                            } else {
                                JsonRpcResponse {
                            jsonrpc: "2.0",
                            id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: 4005,
                                name: "UNKNOWN_SETTING".into(),
                                message: format!(
                                    "Setting '{target}' not found in model schema '{}'",
                                    model.name()
                                ),
                                data: None,
                                hint: Some(
                                    "Run settings.schema without parameters to inspect all fields."
                                        .into(),
                                ),
                            }),
                        }
                            }
                        } else {
                            JsonRpcResponse {
                                jsonrpc: "2.0",
                                id,
                                result: Some(serde_json::to_value(&specs).unwrap_or_default()),
                                error: None,
                            }
                        }
                    }
                }
            }
            "firmware.inspect" => {
                let file_str = req.params.get("file").and_then(Value::as_str);
                match file_str {
                    Some(f) => match crate::firmware::Firmware::load(Path::new(f)) {
                        Ok(fw) => {
                            if let Some(comp_str) =
                                req.params.get("companion").and_then(Value::as_str)
                            {
                                match crate::firmware::Firmware::load(Path::new(comp_str))
                                    .and_then(|comp| fw.validate_companion(&comp))
                                {
                                    Ok(()) => {}
                                    Err(e) => {
                                        let err_resp = JsonRpcResponse {
                                            jsonrpc: "2.0",
                                            id,
                                            result: None,
                                            error: Some(JsonRpcError::hardware_error(e)),
                                        };
                                        writeln!(stdout, "{}", serde_json::to_string(&err_resp)?)?;
                                        stdout.flush()?;
                                        continue;
                                    }
                                }
                            }
                            JsonRpcResponse {
                                jsonrpc: "2.0",
                                id,
                                result: Some(
                                    serde_json::to_value(&fw.metadata).unwrap_or_default(),
                                ),
                                error: None,
                            }
                        }
                        Err(e) => JsonRpcResponse {
                            jsonrpc: "2.0",
                            id,
                            result: None,
                            error: Some(JsonRpcError::hardware_error(e)),
                        },
                    },
                    None => JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(JsonRpcError::invalid_params(
                            "Missing 'file' parameter",
                            Some("Provide path to firmware container file (.dat)."),
                        )),
                    },
                }
            }
            _ => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(JsonRpcError::method_not_found(method)),
            },
        };

        writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.flush()?;
    }

    // EOF ends the session like `exit`: release an open PC-link.
    if let Some(mut s) = session.take() {
        let _ = s.normal_stop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rpc_error_formatting() {
        let err = JsonRpcError::method_not_found("unknown_cmd");
        assert_eq!(err.code, -32601);
        assert_eq!(err.name, "METHOD_NOT_FOUND");
        assert!(err.hint.is_some());

        let not_conn = JsonRpcError::not_connected(Some("Connect first"));
        assert_eq!(not_conn.code, 4001);
        assert_eq!(not_conn.name, "NOT_CONNECTED");
        assert_eq!(not_conn.hint.as_deref(), Some("Connect first"));

        let hw =
            JsonRpcError::hardware_error(anyhow::anyhow!("motor serial mismatch: expected foo"));
        assert_eq!(hw.code, 4002);
        assert_eq!(hw.name, "SERIAL_MISMATCH");

        let auth_err =
            JsonRpcError::hardware_error(anyhow::anyhow!("challenge authentication failed"));
        assert_eq!(auth_err.code, 4003);
        assert_eq!(auth_err.name, "AUTH_FAILED");
    }

    #[test]
    fn probe_target_rejects_selectors_it_would_otherwise_ignore() {
        assert_eq!(probe_target(&json!({})), Ok((None, None)));
        assert_eq!(probe_target(&json!({"slot": null})), Ok((None, None)));
        assert_eq!(
            probe_target(&json!({"slot": 13, "component": "ST-6870-L"})),
            Ok((Some(13), Some("ST-6870-L".into())))
        );
        for slot in [json!(32), json!(269), json!(-1), json!("13"), json!(1.5)] {
            assert!(probe_target(&json!({ "slot": slot })).is_err(), "{slot}");
        }
        for component in [json!(5), json!(["ST-6870-L"])] {
            assert!(probe_target(&json!({ "component": component })).is_err());
        }
    }

    #[test]
    fn request_response_serialization() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"session.status","params":{}}"#;
        let req: JsonRpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "session.status");
        assert_eq!(req.id, Some(json!(1)));

        let resp = JsonRpcResponse {
            jsonrpc: "2.0",
            id: req.id,
            result: Some(json!({ "connected": true })),
            error: None,
        };
        let out = serde_json::to_string(&resp).unwrap();
        assert!(out.contains(r#""jsonrpc":"2.0""#));
        assert!(out.contains(r#""connected":true"#));
        assert!(!out.contains(r#""error""#));
    }

    /// One connection to the modeled SM-PCE02 and EP800-CRG, whose motor
    /// answers 16/7C outside a PC-link only in the first discovery after an
    /// adapter reset, as the hardware did.
    #[cfg(unix)]
    #[test]
    fn units_repeat_and_interleave_with_probes_on_one_connection() {
        use crate::device::tests::ModeledBus;
        use std::io::Read;
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!(
            "cranklink-session-units-{}.sock",
            std::process::id()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let device = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bus = ModeledBus::default();
            let mut decoder = crate::protocol::Decoder::default();
            let mut bytes = [0; 4096];
            while let Ok(n @ 1..) = stream.read(&mut bytes) {
                for request in decoder.feed(&bytes[..n]) {
                    for reply in bus.respond(&request.unwrap()) {
                        let encoded = crate::protocol::encode(reply.control, &reply.payload);
                        stream.write_all(&encoded).unwrap();
                    }
                }
            }
            bus
        });
        let port = format!("sim:{}", path.display());
        let input: String = [
            json!({"id": 1, "method": "session.connect", "params": {"port": port}}),
            json!({"id": 2, "method": "units"}),
            json!({"id": 3, "method": "units"}),
            json!({"id": 4, "method": "probe"}),
            json!({"id": 5, "method": "units"}),
            json!({"id": 6, "method": "probe", "params": {"component": "DU-EP800-CRG"}}),
            json!({"id": 7, "method": "exit"}),
        ]
        .iter()
        .map(|request| format!("{request}\n"))
        .collect();
        let mut output = Vec::new();
        let result = run_session_io(input.as_bytes(), &mut output, None, None, None);
        std::fs::remove_file(&path).unwrap();
        result.unwrap();
        let responses: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 8, "{responses:#?}");
        for (id, response) in responses.iter().enumerate().skip(1) {
            assert_eq!(response["id"], id, "{response}");
            assert!(response.get("error").is_none(), "{response}");
        }
        for id in [2, 3, 5] {
            assert_eq!(responses[id]["result"]["units"][0]["slot"], 13);
            assert_eq!(responses[id]["result"]["units"][0]["model"], "DU-EP800-CRG");
        }
        for id in [4, 6] {
            assert_eq!(responses[id]["result"]["serial"], "010203040506");
        }
        // `exit` closed the connection, which ends the device thread.
        let bus = device.join().unwrap();
        // Connect, then the repeated inventories and the named probe.
        assert_eq!(bus.resets, 4);
        assert_eq!(bus.unanswered_attribute_reads, 0);
    }
}
