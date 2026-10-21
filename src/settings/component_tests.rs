use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn component_catalog_schemas_have_unique_names_and_valid_wire_layouts() {
    for entry in crate::unit_models::UNIT_MODELS
        .iter()
        .filter(|entry| !entry.name.starts_with("DU-"))
    {
        let model = SettingsModel::from_name(entry.name).unwrap();
        let specs = schema_for_model(model);
        let mut names = std::collections::BTreeSet::new();
        for spec in &specs {
            assert!(
                names.insert(spec.name.clone()),
                "duplicate {} in {}",
                spec.name,
                entry.name
            );
            assert!(
                spec.offset + spec.width <= spec.template.len(),
                "{} {}",
                entry.name,
                spec.name
            );
            assert!(
                meaningful_len(spec) <= spec.template.len(),
                "{} {}",
                entry.name,
                spec.name
            );
        }
        assert_eq!(
            capabilities_for_model(model)["settings_available"],
            !specs.is_empty()
        );
    }
    assert!(schema_for_model(SettingsModel::from_name("SM-PCE1").unwrap()).is_empty());
    assert!(SettingsModel::from_name("RD-UNKNOWN").is_err());
}

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "cranklink-component-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn backup(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn hub_adjustment_waits_for_actuator_readback_without_repeating_set() {
    struct Hub {
        reads: usize,
        writes: usize,
        bad_maximum: bool,
        target: u8,
    }
    impl UnitCommands for Hub {
        fn command(
            &mut self,
            _: u8,
            group: u8,
            command: u8,
            params: &[u8],
            _: u8,
        ) -> Result<Vec<u8>> {
            match (group, command) {
                (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                (0x0b, 0x14) => {
                    self.reads += 1;
                    if self.writes == 0 {
                        return Ok(vec![0, 4]);
                    }
                    if self.bad_maximum {
                        return Ok(vec![0, 5]);
                    }
                    Ok(vec![if self.reads >= 3 { self.target } else { 0 }, 4])
                }
                (0x0b, 0x10) => {
                    assert_eq!(params, [self.target, 4, 0, 0]);
                    self.writes += 1;
                    Ok(vec![0]) // Accepted, actuator has not yet completed.
                }
                _ => panic!("unexpected hub command"),
            }
        }
    }
    for (bad_maximum, target) in [(false, 2), (true, 2), (false, 0)] {
        let files = Files::new();
        let backup = files.backup("hub.json");
        let mut hub = Hub {
            reads: 0,
            writes: 0,
            bad_maximum,
            target,
        };
        let result = set_for_model(
            &mut hub,
            7,
            "hub-adjustment",
            &target.to_string(),
            &backup,
            SettingsModel::Component("MU-UR500"),
        );
        if bad_maximum {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("invalid setting readback"));
            assert_eq!(hub.reads, 2);
        } else {
            let receipt = result.unwrap();
            assert_eq!(receipt.after, target);
            assert_eq!(receipt.changed, target != 0);
            assert_eq!(hub.reads, if target == 0 { 2 } else { 3 });
        }
        assert_eq!(hub.writes, 1);
        assert!(backup.exists());
    }
}

struct Switch {
    state: Vec<u8>,
    backup: PathBuf,
    calls: usize,
    writes: usize,
    corrupt_sibling: bool,
    truncate_readback: bool,
}
impl Switch {
    fn new(backup: PathBuf) -> Self {
        Self {
            // Classic GET replies may pad the meaningful AB/CS prefix with FF.
            state: vec![0x23, 0xaf, 0xff, 0xff],
            backup,
            calls: 0,
            writes: 0,
            corrupt_sibling: false,
            truncate_readback: false,
        }
    }
}
impl UnitCommands for Switch {
    fn command(
        &mut self,
        slot: u8,
        group: u8,
        command: u8,
        params: &[u8],
        expected: u8,
    ) -> Result<Vec<u8>> {
        assert_eq!(slot, 7);
        self.calls += 1;
        match (group, command, expected) {
            (1, 0x3c, 0x3e) => {
                assert_eq!(params, [0]);
                Ok(vec![1, 2, 3, 4, 5, 6])
            }
            (4, 0x14, 0x16) => {
                assert_eq!(params, [0; 4]);
                let mut state = self.state.clone();
                if self.truncate_readback && self.writes > 0 {
                    state.truncate(1);
                }
                Ok(state)
            }
            (4, 0x10, 0x12) => {
                // Backup must exist with the complete original bytes and model
                // identity before the first mutable command reaches hardware.
                let backup: Value = serde_json::from_slice(&std::fs::read(&self.backup)?)?;
                assert_eq!(backup["serial"], "010203040506");
                assert_eq!(backup["settings_model"], "ST-9070-L");
                assert_eq!(backup["original_parameters"], hex::encode(&self.state));
                assert_eq!(backup["planned_parameters"], hex::encode(params));
                assert_eq!(backup["setting"], "switch-a-function");
                assert_eq!(params.len(), 4);
                assert_eq!(params[2..], [0, 0]);
                self.writes += 1;
                // SET reserved bytes are zero; GET padding remains independent.
                self.state[..2].copy_from_slice(&params[..2]);
                if self.corrupt_sibling {
                    self.state[0] ^= 1;
                }
                Ok(Vec::new())
            }
            _ => anyhow::bail!("unexpected command {group:02x}/{command:02x}"),
        }
    }
}

#[test]
fn packed_component_write_and_restore_bind_model_and_preserve_current_siblings() {
    let files = Files::new();
    let backup = files.backup("before.json");
    let restored_backup = files.backup("restore-before.json");
    let model = SettingsModel::Component("ST-9070-L");
    let mut switch = Switch::new(backup.clone());
    let receipt = set_for_model(&mut switch, 7, "switch-a-function", "1", &backup, model).unwrap();
    assert_eq!(receipt.before, json!(2));
    assert_eq!(receipt.after, json!(1));
    assert_eq!(switch.state, [0x13, 0xaf, 0xff, 0xff]);

    // Independent changes made after the backup must survive restoration.
    switch.state = vec![0x10, 0xbc, 0xff, 0x6a];
    switch.backup = restored_backup.clone();
    restore_for_model(&mut switch, 7, &backup, &restored_backup, model).unwrap();
    assert_eq!(switch.state, [0x20, 0xbc, 0xff, 0x6a]);
    assert_eq!(switch.writes, 2);
    let parsed = parse_restore_file(&backup).unwrap();
    assert!(parsed
        .validate_for_model(SettingsModel::Component("ST-9070-R"), "010203040506")
        .is_err());
    assert!(parsed.validate_for_model(model, "010203040507").is_err());
}

#[test]
fn component_readback_detects_same_byte_sibling_corruption() {
    let files = Files::new();
    let backup = files.backup("before.json");
    let mut switch = Switch::new(backup.clone());
    switch.corrupt_sibling = true;
    let error = set_for_model(
        &mut switch,
        7,
        "switch-a-function",
        "1",
        &backup,
        SettingsModel::Component("ST-9070-L"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("shared setting bits changed"));
    assert_eq!(switch.writes, 1);
    assert!(backup.exists());
}

#[test]
fn component_write_rejects_truncated_classic_switch_prefix_before_backup_or_mutation() {
    let files = Files::new();
    let backup = files.backup("before.json");
    let mut switch = Switch::new(backup.clone());
    switch.state.truncate(1);
    assert!(set_for_model(
        &mut switch,
        7,
        "switch-a-function",
        "1",
        &backup,
        SettingsModel::Component("ST-9070-L"),
    )
    .is_err());
    assert_eq!(switch.writes, 0);
    assert!(!backup.exists());
}

#[test]
fn component_write_accepts_unpadded_classic_switch_prefix() {
    let files = Files::new();
    let backup = files.backup("before.json");
    let mut switch = Switch::new(backup.clone());
    switch.state.truncate(2);
    let receipt = set_for_model(
        &mut switch,
        7,
        "switch-a-function",
        "1",
        &backup,
        SettingsModel::Component("ST-9070-L"),
    )
    .unwrap();
    assert_eq!(receipt.before, json!(2));
    assert_eq!(receipt.after, json!(1));
    assert_eq!(switch.state, [0x13, 0xaf]);
    assert_eq!(switch.writes, 1);
}

#[test]
fn component_write_preserves_original_assignments_that_cannot_be_restored() {
    let files = Files::new();
    let backup = files.backup("before.json");
    let mut switch = Switch::new(backup.clone());
    switch.state[0] = 0xa3; // D-FLY is readable but outside the remapping choices.
    let error = set_for_model(
        &mut switch,
        7,
        "switch-a-function",
        "1",
        &backup,
        SettingsModel::Component("ST-9070-L"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("backup cannot be restored"));
    assert_eq!(switch.writes, 0);
    assert!(!backup.exists());
    assert_eq!(switch.state[0], 0xa3);
}

#[test]
fn component_write_rejects_truncated_readback() {
    let files = Files::new();
    let backup = files.backup("before.json");
    let mut switch = Switch::new(backup.clone());
    switch.truncate_readback = true;
    assert!(set_for_model(
        &mut switch,
        7,
        "switch-a-function",
        "1",
        &backup,
        SettingsModel::Component("ST-9070-L"),
    )
    .is_err());
    assert_eq!(switch.writes, 1);
    assert!(backup.exists());
}

#[test]
fn unsupported_and_read_only_component_settings_fail_before_io() {
    let files = Files::new();
    let backup = files.backup("unused.json");
    let mut switch = Switch::new(backup.clone());
    for (model, setting) in [
        ("ST-R9270-L", "switch-a-function"),
        ("ST-9070-L", "switch-c-function"),
        ("RD-M8050-GS", "derailleur-adjustment"),
        ("RD-X8050", "multi-shift-speed"),
    ] {
        assert!(set_for_model(
            &mut switch,
            7,
            setting,
            "1",
            &backup,
            SettingsModel::Component(model),
        )
        .is_err());
    }
    assert_eq!(switch.calls, 0);
    assert!(!backup.exists());
}
