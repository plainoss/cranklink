use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "cranklink-batch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn prefix(&self) -> PathBuf {
        self.0.join("run")
    }
    fn journal(&self) -> Vec<Value> {
        std::fs::read_to_string(suffixed(&self.prefix(), ".journal.jsonl"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Motor {
    prefix: PathBuf,
    speed: u16,
    destination: u8,
    max_power: [u16; 3],
    writes: usize,
    fail_write: Option<usize>,
    mismatch: bool,
}
impl Motor {
    fn new(files: &Files) -> Self {
        Self {
            prefix: files.prefix(),
            speed: 2800,
            destination: 1,
            max_power: [400; 3],
            writes: 0,
            fail_write: None,
            mismatch: false,
        }
    }
}
impl UnitCommands for Motor {
    fn command(
        &mut self,
        slot: u8,
        group: u8,
        command: u8,
        params: &[u8],
        _: u8,
    ) -> Result<Vec<u8>> {
        assert_eq!(slot, 13);
        match (group, command) {
            (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
            (0x16, 0xb4) => {
                let n = if self.mismatch && self.writes > 0 {
                    2800
                } else {
                    self.speed
                };
                Ok(vec![n as u8, (n >> 8) as u8, 0xff, 0xff])
            }
            (0x16, 0xac) => Ok(vec![1, self.destination, 0xff, 0xff]),
            (0x17, 0x14) => {
                let watts = self.max_power[params[0] as usize - 1].to_le_bytes();
                Ok(vec![params[0], watts[0], watts[1]])
            }
            (0x16, 0xb0 | 0xa8) => {
                // Verify backup and durable journal intent exist when asked to write.
                let rows: Vec<Value> =
                    std::fs::read_to_string(suffixed(&self.prefix, ".journal.jsonl"))?
                        .lines()
                        .map(|s| serde_json::from_str(s).unwrap())
                        .collect();
                let attempt = rows.last().unwrap();
                assert_eq!(attempt["stage"], "attempt");
                let backup: Value =
                    serde_json::from_slice(&std::fs::read(attempt["backup"].as_str().unwrap())?)?;
                assert_eq!(backup["serial"], "010203040506");
                if command == 0xb0 {
                    assert_eq!(backup["before"], json!(self.speed as f64 / 100.0));
                    self.speed = u16::from_le_bytes([params[0], params[1]]);
                } else {
                    assert_eq!(backup["before"], self.destination);
                    self.destination = params[1];
                }
                self.writes += 1;
                if self.fail_write == Some(self.writes) {
                    anyhow::bail!("simulated lost write acknowledgment");
                }
                Ok(vec![])
            }
            _ => anyhow::bail!("unexpected command {group:02x}/{command:02x}"),
        }
    }
}
fn operations(value: Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

#[test]
fn ordered_roundtrip_backs_up_each_write_and_reads_final_state() {
    let files = Files::new();
    let mut motor = Motor::new(&files);
    let ops = operations(json!([
        {"op":"set","name":"speed-limit","value":25},
        {"op":"set","name":"destination","value":"0"},
        {"op":"set","name":"destination","value":1},
        {"op":"set","name":"speed-limit","value":28},
        {"op":"read","name":"destination"},
        {"op":"read","name":"speed-limit"}
    ]));
    let report = run(
        &mut motor,
        13,
        &ops,
        &files.prefix(),
        SettingsModel::Ep800Crg,
    )
    .unwrap();
    assert_eq!(motor.writes, 4);
    assert_eq!(report["completed"], 6);
    assert_eq!(report["results"][4]["value"], 1);
    assert_eq!(report["results"][5]["value"], 28.0);
    assert_eq!(
        files
            .journal()
            .iter()
            .filter(|v| v["stage"] == "completed")
            .count(),
        6
    );
    assert_eq!(
        files.journal().last().unwrap()["stage"],
        "operations-completed"
    );
}

#[test]
fn uncertain_write_stops_plan_without_retry_or_rollback() {
    let files = Files::new();
    let mut motor = Motor::new(&files);
    motor.fail_write = Some(2);
    let ops = operations(json!([
        {"op":"set","name":"speed-limit","value":25},
        {"op":"set","name":"destination","value":0},
        {"op":"set","name":"destination","value":1}
    ]));
    let error = run(
        &mut motor,
        13,
        &ops,
        &files.prefix(),
        SettingsModel::Ep800Crg,
    )
    .unwrap_err();
    assert!(error.to_string().contains("operation 2; 1 completed"));
    assert!(error.to_string().contains("outcome may be uncertain"));
    assert_eq!(motor.writes, 2);
    let rows = files.journal();
    assert_eq!(rows.last().unwrap()["stage"], "failed");
    assert_eq!(rows.last().unwrap()["operation"], 2);
    assert!(suffixed(&files.prefix(), ".002.backup.json").exists());
    assert!(!suffixed(&files.prefix(), ".003.backup.json").exists());
}

#[test]
fn readback_mismatch_stops_before_next_write() {
    let files = Files::new();
    let mut motor = Motor::new(&files);
    motor.mismatch = true;
    let ops = operations(json!([
        {"op":"set","name":"speed-limit","value":25},
        {"op":"set","name":"destination","value":0}
    ]));
    let error = run(
        &mut motor,
        13,
        &ops,
        &files.prefix(),
        SettingsModel::Ep800Crg,
    )
    .unwrap_err();
    assert!(error.to_string().contains("readback mismatch"));
    assert_eq!(motor.writes, 1);
    assert_eq!(files.journal().last().unwrap()["stage"], "failed");
}

#[test]
fn invalid_later_value_is_rejected_before_any_batch_write() {
    let files = Files::new();
    let mut motor = Motor::new(&files);
    let ops = operations(json!([
        {"op":"set","name":"speed-limit","value":25},
        {"op":"set","name":"destination","value":99}
    ]));
    assert!(run(
        &mut motor,
        13,
        &ops,
        &files.prefix(),
        SettingsModel::Ep800Crg
    )
    .unwrap_err()
    .to_string()
    .contains("operation 2 preflight"));
    assert_eq!(motor.writes, 0);
    assert!(!suffixed(&files.prefix(), ".journal.jsonl").exists());
}

#[test]
fn existing_backup_is_never_overwritten_and_remaining_steps_stop() {
    let files = Files::new();
    let collision = suffixed(&files.prefix(), ".002.backup.json");
    std::fs::write(&collision, b"keep me").unwrap();
    let mut motor = Motor::new(&files);
    let ops = operations(json!([
        {"op":"set","name":"speed-limit","value":25},
        {"op":"set","name":"destination","value":0}
    ]));
    assert!(run(
        &mut motor,
        13,
        &ops,
        &files.prefix(),
        SettingsModel::Ep800Crg
    )
    .is_err());
    assert_eq!(motor.writes, 1);
    assert_eq!(std::fs::read(collision).unwrap(), b"keep me");
    assert_eq!(files.journal().last().unwrap()["operation"], 2);
}

#[test]
fn restore_uses_existing_serial_bound_backup_and_saves_current_value() {
    let files = Files::new();
    let original = files.0.join("original.json");
    std::fs::write(&original, serde_json::to_vec(&json!({"format":"esteps-setting-backup-v1","serial":"010203040506","setting":"speed-limit","before":28.0,"original_parameters":"f00affff"})).unwrap()).unwrap();
    let mut motor = Motor::new(&files);
    motor.speed = 2500;
    let report = run(
        &mut motor,
        13,
        &[Operation::Restore {
            file: original,
            parsed: None,
        }],
        &files.prefix(),
        SettingsModel::Ep800Crg,
    )
    .unwrap();
    assert_eq!(motor.speed, 2800);
    assert_eq!(report["results"][0]["before"], 25.0);
    assert_eq!(report["results"][0]["after"], 28.0);
}

#[test]
fn loader_rejects_unknown_fields_and_empty_plan_and_resolves_restore_paths() {
    let files = Files::new();
    let plan = files.0.join("plan.json");
    for invalid in [
        r#"[]"#,
        r#"[{"op":"set","name":"lamp","value":true}]"#,
        r#"[{"op":"read","naem":"lamp"}]"#,
    ] {
        std::fs::write(&plan, invalid).unwrap();
        assert!(load(&plan).is_err());
    }
    std::fs::write(&plan, r#"[{"op":"restore","file":"before.json"}]"#).unwrap();
    assert!(
        matches!(&load(&plan).unwrap()[0], Operation::Restore { file, .. } if file == &files.0.join("before.json"))
    );
}

#[test]
fn invalid_restore_input_prevents_earlier_valid_writes() {
    for (serial, before, raw) in [
        ("aaaaaaaaaaaa", 28.0, "f00affff"),
        ("010203040506", 99.0, "ac26ffff"),
    ] {
        let files = Files::new();
        let original = files.0.join("original.json");
        std::fs::write(&original, serde_json::to_vec(&json!({"format":"esteps-setting-backup-v1","serial":serial,"setting":"speed-limit","before":before,"original_parameters":raw})).unwrap()).unwrap();
        let mut motor = Motor::new(&files);
        let ops = operations(json!([
            {"op":"set","name":"speed-limit","value":25},
            {"op":"restore","file":original}
        ]));
        let error = run(
            &mut motor,
            13,
            &ops,
            &files.prefix(),
            SettingsModel::Ep800Crg,
        )
        .unwrap_err();
        assert!(error.to_string().contains("operation 2 preflight"));
        assert_eq!(motor.writes, 0);
        assert!(!suffixed(&files.prefix(), ".journal.jsonl").exists());
    }
}

fn attempts(files: &Files) -> Vec<u64> {
    files
        .journal()
        .iter()
        .filter(|row| row["stage"] == "attempt")
        .map(|row| row["operation"].as_u64().unwrap())
        .collect()
}

#[test]
fn region_changes_listed_region_first_run_in_a_valid_order_and_restore() {
    // (destination, speed) before, the plan's new pair, and the expected
    // execution order of [destination, speed-limit].
    for (before, after, order) in [((1, 3200), (0, 25), [2, 1]), ((0, 2500), (1, 32), [1, 2])] {
        let files = Files::new();
        let mut motor = Motor::new(&files);
        (motor.destination, motor.speed) = before;
        let report = run(
            &mut motor,
            13,
            &operations(json!([
                {"op":"set","name":"destination","value":after.0},
                {"op":"set","name":"speed-limit","value":after.1}
            ])),
            &files.prefix(),
            SettingsModel::Ep800Crg,
        )
        .unwrap();
        assert_eq!(attempts(&files), order);
        assert_eq!(files.journal()[0]["order"], json!(order));
        assert_eq!((motor.destination, motor.speed), (after.0, after.1 * 100));
        assert_eq!(report["results"][0]["setting"], "destination");
        assert_eq!(report["results"][1]["setting"], "speed-limit");

        // Restoring the save's backups in the same forward order undoes it.
        let undo = Files::new();
        motor.prefix = undo.prefix();
        let backups: Vec<_> = (0..2)
            .map(|i| json!({"op":"restore","file":report["results"][i]["backup"]}))
            .collect();
        run(
            &mut motor,
            13,
            &operations(json!(backups)),
            &undo.prefix(),
            SettingsModel::Ep800Crg,
        )
        .unwrap();
        assert_eq!(attempts(&undo), [order[1], order[0]]);
        assert_eq!((motor.destination, motor.speed), before);
    }
}

#[test]
fn unsatisfied_region_change_is_refused_before_any_write() {
    for (before, max_power, plan, refusal) in [
        // The speed fits both regions, but boost power does not fit region 3.
        (
            (1, 2800),
            [400, 400, 500],
            json!([
                {"op":"set","name":"destination","value":3},
                {"op":"set","name":"speed-limit","value":25}
            ]),
            "change max-power.boost before destination 3",
        ),
        // A read between them keeps the region write first.
        (
            (1, 3200),
            [400; 3],
            json!([
                {"op":"set","name":"destination","value":0},
                {"op":"read","name":"speed-limit"},
                {"op":"set","name":"speed-limit","value":25}
            ]),
            "change speed-limit before destination 0",
        ),
        // A limit listed before its region is not moved after it.
        (
            (0, 2500),
            [400; 3],
            json!([
                {"op":"set","name":"speed-limit","value":32},
                {"op":"set","name":"destination","value":1}
            ]),
            "destination 0 permits 15..25 km/h",
        ),
        // Neither order fits: the old speed is below the new region and the
        // new speed above the old one, in both directions.
        (
            (0, 1500),
            [400; 3],
            json!([
                {"op":"set","name":"destination","value":1},
                {"op":"set","name":"speed-limit","value":30}
            ]),
            "outside destination 0 too (destination 0 permits 15..25 km/h), so first save one both destinations permit: destination 1 permits 19..32 km/h",
        ),
        (
            (1, 3000),
            [400; 3],
            json!([
                {"op":"set","name":"destination","value":0},
                {"op":"set","name":"speed-limit","value":15}
            ]),
            "outside destination 1 too (destination 1 permits 19..32 km/h)",
        ),
    ] {
        let files = Files::new();
        let mut motor = Motor::new(&files);
        (motor.destination, motor.speed) = before;
        motor.max_power = max_power;
        let error = run(
            &mut motor,
            13,
            &operations(plan),
            &files.prefix(),
            SettingsModel::Ep800Crg,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("preflight failed; no batch writes sent"));
        assert!(format!("{error:#}").contains(refusal), "{error:#}");
        assert_eq!(motor.writes, 0);
        assert!(!suffixed(&files.prefix(), ".journal.jsonl").exists());
    }
}

#[test]
fn region_change_never_reorders_writes_of_the_same_setting() {
    let files = Files::new();
    let mut motor = Motor::new(&files);
    (motor.destination, motor.speed) = (0, 2000);
    run(
        &mut motor,
        13,
        &operations(json!([
            {"op":"set","name":"destination","value":1},
            {"op":"set","name":"speed-limit","value":32},
            {"op":"set","name":"speed-limit","value":25}
        ])),
        &files.prefix(),
        SettingsModel::Ep800Crg,
    )
    .unwrap();
    assert_eq!(attempts(&files), [1, 2, 3]);
    assert_eq!((motor.destination, motor.speed), (1, 2500));
}
