//! Narrow JNI entry point for the Android E-STEPS app. All protocol and
//! durable backup work stays in the same Rust core as the web app.
mod java_gatt;

use anyhow::{bail, ensure, Context, Result};
use cranklink::{
    ble::{BleSession, SystemDetection},
    device::{Identity, UnitCommands, UnitIdentity},
    motor_support,
    settings::{self, batch::Operation, ParsedRestore, SettingSpec, SettingsModel},
};
use jni::{
    objects::{GlobalRef, JObject, JString},
    sys::jstring,
    JNIEnv,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
static BACKUP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// No CLI dispatch, component selector, raw command, USB, or firmware operation
/// can be represented by this request type.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum Request {
    Connect {
        #[serde(default = "default_master_routing")]
        master: bool,
    },
    Read {
        serial: String,
    },
    Commit {
        serial: String,
        changes: BTreeMap<String, String>,
        #[serde(default)]
        master: bool,
    },
    Restore {
        serial: String,
        file: String,
        #[serde(default)]
        master: bool,
    },
    Disconnect {},
    Backups {},
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Motor {
    serial: String,
    model: SettingsModel,
    model_name: String,
    series: u8,
    unit: u8,
    params: Vec<u8>,
    slot: u8,
    dcas_version: String,
    rx_version: String,
}

impl Motor {
    fn json(&self, master: bool) -> Value {
        json!({
            "model": self.model_name, "model_name": self.model_name,
            "serial": self.serial, "slot": self.slot,
            "dcas_version": self.dcas_version, "rx_version": self.rx_version,
            "settings_model": self.model.slug(), "model_series": self.series,
            "model_unit": self.unit, "master_routing": master
        })
    }

    fn check_same(&self, live: &Self) -> Result<()> {
        ensure!(
            self == live,
            "motor identity changed; disconnect and read the selected bike again before saving"
        );
        Ok(())
    }
}

struct Active {
    session: BleSession,
    motor: Motor,
    java_gatt: GlobalRef,
}

// Kotlin calls JNI only from one dedicated executor. The session never crosses
// Rust threads, and GATT callbacks remain on Android's separate callback thread.
thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

fn parse_request(request: &str) -> Result<Request> {
    ensure!(
        request.len() <= MAX_REQUEST_BYTES,
        "request exceeds size limit"
    );
    let request = serde_json::from_str(request).context("invalid Android app request")?;
    ensure!(
        !matches!(request, Request::Connect { master: false }),
        "the STEPS app requires master BLE routing"
    );
    Ok(request)
}

fn default_master_routing() -> bool {
    true
}

fn backup_directory(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    ensure!(path.is_absolute(), "backup directory must be absolute");
    std::fs::create_dir_all(path).context("create private backup directory")?;
    std::fs::canonicalize(path).context("resolve private backup directory")
}

fn list_backups(directory: &Path) -> Result<Vec<String>> {
    let mut backups = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            if let Some(name) = entry.file_name().to_str() {
                if valid_backup_name(name) {
                    backups.push(name.to_owned());
                }
            }
        }
    }
    backups.sort_unstable_by(|a, b| b.cmp(a));
    Ok(backups)
}

fn valid_backup_name(name: &str) -> bool {
    name.len() <= 240
        && name.ends_with(".backup.json")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && !name.starts_with('.')
}

fn restore_path(directory: &Path, filename: &str) -> Result<PathBuf> {
    ensure!(
        valid_backup_name(filename),
        "restore requires a private backup basename"
    );
    let path = directory.join(filename);
    ensure!(
        std::fs::symlink_metadata(&path)?.file_type().is_file(),
        "restore input must be a regular private backup file"
    );
    ensure!(
        std::fs::canonicalize(&path)?.parent() == Some(directory),
        "restore file escapes private backup directory"
    );
    Ok(path)
}

fn backup_prefix(directory: &Path, serial: &str) -> PathBuf {
    loop {
        let counter = BACKUP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "backup-{serial}-{}-{counter:06}",
            cranklink::platform::now_ms()
        );
        if !directory.join(format!("{name}.journal.jsonl")).exists() {
            return directory.join(name);
        }
    }
}

fn read_identity(session: &mut BleSession, identity: &Identity) -> Result<Motor> {
    let stock = UnitIdentity::from_reply(identity.slot, session.model_identity(identity.slot)?)?;
    stock.require_motor()?;
    let params = motor_support::read_params(session, identity.slot, stock.model_series)?;
    let model = motor_support::settings_for_motor(
        stock.model_series,
        stock.model_unit,
        &params,
        &identity.dcas_version,
    )?;
    ensure!(
        !matches!(model, SettingsModel::Component(_)),
        "E-STEPS drive unit required"
    );
    let serial = hex::encode(cranklink::device::serial_bytes(&session.command(
        identity.slot,
        1,
        0x3c,
        &[0],
        0x3e,
    )?)?);
    let model_name =
        cranklink::unit_models::identify(stock.model_series, stock.model_unit, None, Some(&params))
            .unwrap_or(model.name())
            .to_owned();
    Ok(Motor {
        serial,
        model,
        model_name,
        series: stock.model_series,
        unit: stock.model_unit,
        params,
        slot: identity.slot,
        dcas_version: identity.dcas_version.clone(),
        rx_version: identity.rx_version.clone(),
    })
}

fn spec<'a>(schema: &'a [SettingSpec], name: &str) -> Result<&'a SettingSpec> {
    schema
        .iter()
        .find(|s| s.name == name)
        .with_context(|| format!("setting {name:?} is outside this motor's E-STEPS tuning schema"))
}

fn preflight_changes(schema: &[SettingSpec], changes: &BTreeMap<String, String>) -> Result<()> {
    ensure!(
        !changes.is_empty() && changes.len() <= 128,
        "save requires 1..128 staged changes"
    );
    for (name, value) in changes {
        spec(schema, name)?
            .validate(value)
            .with_context(|| format!("invalid staged value for {name}; no writes sent"))?;
    }
    Ok(())
}

fn preflight_restore(parsed: &ParsedRestore, motor: &Motor, schema: &[SettingSpec]) -> Result<()> {
    spec(schema, &parsed.setting)?;
    ensure!(
        parsed.raw_backup["settings_model"].as_str() == Some(motor.model.slug()),
        "backup belongs to another motor model or lacks its model identity"
    );
    parsed.validate_for_model(motor.model, &motor.serial)?;
    Ok(())
}

fn modern(model: SettingsModel) -> bool {
    matches!(
        model,
        SettingsModel::Ep600 | SettingsModel::Ep801 | SettingsModel::Ep500
    )
}

/// Read-only OEM evidence for modern basic-mode controls. Unknown bounds are
/// returned as evidence, never interpolated into an assumed list of levels.
fn torque_constraints(
    session: &mut dyn UnitCommands,
    slot: u8,
    model: SettingsModel,
) -> Option<Value> {
    if !modern(model) {
        return None;
    }
    let reply = session.command(slot, 0x17, 0x1c, &[0], 0x1e).ok()?;
    (reply.len() >= 2).then(|| json!({"torque_minimum":reply[0], "torque_maximum":reply[1]}))
}

fn snapshot(active: &mut Active, directory: &Path, report: Option<Value>) -> Result<Value> {
    let snapshot =
        settings::read_ebike_for_model(&mut active.session, active.motor.slot, active.motor.model);
    let constraints =
        torque_constraints(&mut active.session, active.motor.slot, active.motor.model);
    // Incomplete schema reads are reported per field. Any transport failure also
    // poisons the session, and pause below then rejects retention of that link.
    let mut result = json!({
        "identity": active.motor.json(active.session.is_master_routing()),
        "schema": settings::ebike_schema_for_model(active.motor.model),
        "snapshot": snapshot,
        "backups": list_backups(directory)?
    });
    if let Some(constraints) = constraints {
        result["constraints"] = constraints;
    }
    if let Some(report) = report {
        result["report"] = report;
    }
    active
        .session
        .pause_settings_command()
        .context("release motor after settings command")?;
    Ok(result)
}

/// Older STEPS masters are not all catalogued, so an unrecognized family
/// proceeds to the motor identity checks; only a Di2 power unit is refused.
fn require_steps_system(detection: &SystemDetection) -> Result<()> {
    ensure!(
        detection.system != "shifting-only",
        "this bike reports a Di2 shifting system without a STEPS drive unit; the Android app supports STEPS drive units only"
    );
    Ok(())
}

fn close_active() -> Result<()> {
    ACTIVE.with(|cell| {
        let Some(mut active) = cell.borrow_mut().take() else {
            return Ok(());
        };
        let stopped = active.session.stop();
        let closed = active.session.close();
        stopped.context("motor PC-link cleanup was not confirmed")?;
        closed.context("close Android GATT")
    })
}

fn cleanup_error(session: &mut BleSession, error: anyhow::Error) -> anyhow::Error {
    let stopped = session.stop();
    let closed = session.close();
    match (stopped, closed) {
        (Ok(()), Ok(())) => error,
        (Err(cleanup), _) | (_, Err(cleanup)) => {
            error.context(format!("cleanup could not be confirmed: {cleanup:#}"))
        }
    }
}

fn execute(
    env: &mut JNIEnv<'_>,
    gatt: &JObject<'_>,
    request: Request,
    directory: &Path,
) -> Result<Value> {
    if matches!(request, Request::Backups {}) {
        return Ok(json!({"backups":list_backups(directory)?}));
    }
    if matches!(request, Request::Disconnect {}) {
        close_active()?;
        return Ok(json!({"disconnected":true,"backups":list_backups(directory)?}));
    }
    if let Request::Connect { master } = request {
        close_active()?;
        let java_gatt = env.new_global_ref(gatt)?;
        let io = java_gatt::JavaGatt::new(env, java_gatt.clone())?;
        let mut session = BleSession::new(Box::new(io), None)?;
        let connected = (|| -> Result<Motor> {
            session.set_master_routing(master)?;
            session.authenticate()?;
            // Read the gateway's master cache before service mode so a
            // shifting-only system never enters master routing.
            require_steps_system(&session.detect_system()?)?;
            let identity = session.start(None)?;
            let motor = read_identity(&mut session, &identity)?;
            if modern(motor.model) {
                session.authenticate_unit_settings(&motor.serial)?;
            }
            Ok(motor)
        })();
        let motor = connected.map_err(|error| cleanup_error(&mut session, error))?;
        let mut active = Active {
            session,
            motor,
            java_gatt,
        };
        let result = snapshot(&mut active, directory, None)
            .map_err(|error| cleanup_error(&mut active.session, error))?;
        ACTIVE.with(|cell| *cell.borrow_mut() = Some(active));
        return Ok(result);
    }
    ACTIVE.with(|cell| {
        let mut state = cell.borrow_mut();
        let active = state
            .as_mut()
            .context("connect and read a supported E-STEPS motor first")?;
        let expected_serial = match &request {
            Request::Read { serial }
            | Request::Commit { serial, .. }
            | Request::Restore { serial, .. } => serial,
            _ => bail!("unsupported request in active settings session"),
        };
        ensure!(
            expected_serial.eq_ignore_ascii_case(&active.motor.serial),
            "request belongs to another motor; reconnect and read the selected bike"
        );
        ensure!(
            env.is_same_object(gatt, active.java_gatt.as_obj())?,
            "GATT connection changed; reconnect before another command"
        );
        let schema = settings::ebike_schema_for_model(active.motor.model);
        let (needs_master, needs_auth, operations) = match request {
            Request::Read { .. } => (false, modern(active.motor.model), Vec::new()),
            Request::Commit {
                changes, master, ..
            } => {
                preflight_changes(&schema, &changes)?;
                let needs_master = master
                    || changes
                        .keys()
                        .any(|name| settings::requires_ble_master_routing(name));
                let needs_auth = modern(active.motor.model)
                    || changes
                        .keys()
                        .any(|name| spec(&schema, name).is_ok_and(|s| s.oem));
                // Give the core a deterministic plan. Its batch planner remains
                // authoritative and moves safe limit reductions before a region
                // change when required; every write revalidates the live state.
                let mut ordered: Vec<_> = changes.into_iter().collect();
                ordered.sort_by_key(|(name, _)| {
                    (if name == "destination" { 0 } else { 1 }, name.clone())
                });
                let operations = ordered
                    .into_iter()
                    .map(|(name, value)| Operation::Set { name, value })
                    .collect();
                (needs_master, needs_auth, operations)
            }
            Request::Restore { file, master, .. } => {
                let file = restore_path(directory, &file)?;
                let parsed = settings::parse_restore_file(&file)?;
                preflight_restore(&parsed, &active.motor, &schema)?;
                let needs_master = master || settings::requires_ble_master_routing(&parsed.setting);
                let needs_auth = modern(active.motor.model) || spec(&schema, &parsed.setting)?.oem;
                (
                    needs_master,
                    needs_auth,
                    vec![Operation::Restore {
                        file,
                        parsed: Some(parsed),
                    }],
                )
            }
            _ => bail!("unsupported request in active settings session"),
        };
        active
            .session
            .resume_settings_command(needs_master || active.session.is_master_routing())?;
        let identity = active.session.start(Some(active.motor.slot))?;
        let live = read_identity(&mut active.session, &identity)?;
        active.motor.check_same(&live)?;
        active.motor = live.clone();
        // Revalidate against the freshly read live identity before sending keys.
        for operation in &operations {
            if let Operation::Restore {
                parsed: Some(parsed),
                ..
            } = operation
            {
                preflight_restore(parsed, &live, &schema)?;
            }
        }
        if needs_auth {
            active.session.authenticate_unit_settings(&live.serial)?;
        }
        let report = if operations.is_empty() {
            None
        } else {
            let prefix = backup_prefix(directory, &live.serial);
            Some(settings::batch::run_bound(
                &mut active.session,
                live.slot,
                &operations,
                &prefix,
                live.model,
                Some(&live.serial),
            )?)
        };
        // Every saved field already passed core readback; refresh the full
        // exposed tuning snapshot before returning to the UI.
        snapshot(active, directory, report)
    })
}

fn invoke(env: &mut JNIEnv<'_>, gatt: &JObject<'_>, request: &str, directory: &str) -> Value {
    let directory = match backup_directory(directory) {
        Ok(directory) => directory,
        Err(error) => return json!({"error":format!("{error:#}")}),
    };
    let result = parse_request(request).and_then(|request| execute(env, gatt, request, &directory));
    match result {
        Ok(result) => json!({"result":result}),
        Err(error) => {
            let cleanup = close_active()
                .err()
                .map(|e| format!("; cleanup: {e:#}"))
                .unwrap_or_default();
            json!({"error":format!("{error:#}{cleanup}"),"backups":list_backups(&directory).unwrap_or_default()})
        }
    }
}

/// Kotlin `object NativeCore` calls this method from its single I/O executor.
#[no_mangle]
pub extern "system" fn Java_app_plainoss_cranklink_NativeCore_invoke(
    mut env: JNIEnv<'_>,
    _owner: JObject<'_>,
    gatt: JObject<'_>,
    request: JString<'_>,
    backup_dir: JString<'_>,
) -> jstring {
    let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
        let request: String = env.get_string(&request)?.into();
        let directory: String = env.get_string(&backup_dir)?.into();
        Ok(invoke(&mut env, &gatt, &request, &directory))
    }));
    let response = match response {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => json!({"error":format!("{error:#}")}),
        Err(_) => {
            // Cleanup must not unwind across the JNI boundary either.
            let _ = std::panic::catch_unwind(close_active);
            json!({"error":"native settings operation failed; reconnect before continuing"})
        }
    };
    // A Java callback exception must be cleared before returning JSON to Kotlin.
    let _ = env.exception_clear();
    env.new_string(response.to_string())
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests;
