use super::*;

fn motor() -> Motor {
    Motor {
        serial: "010203040506".into(),
        model: SettingsModel::Ep800Crg,
        model_name: "DU-EP800".into(),
        series: 0x23,
        unit: 0,
        params: b"00\0\0\0\0\0".to_vec(),
        slot: 13,
        dcas_version: "4.1.0.0".into(),
        rx_version: "1.0.0.0".into(),
    }
}

#[test]
fn dispatch_cannot_represent_firmware_usb_di2_or_raw_commands() {
    for request in [
        r#"{"op":"flash","file":"motor.bin"}"#,
        r#"{"op":"firmware"}"#,
        r#"{"op":"command","group":22,"command":216}"#,
        r#"{"op":"connect","component":"RD-R8150"}"#,
        r#"{"op":"connect","port":"usb"}"#,
        r#"{"op":"connect","master":false}"#,
        r#"{"op":"read","serial":"010203040506","slot":4}"#,
        r#"{"op":"commit","serial":"010203040506","changes":{"lamp":1}}"#,
        r#"{"op":"disconnect","slot":4}"#,
        r#"{"op":"backups","file":"outside.json"}"#,
    ] {
        assert!(parse_request(request).is_err(), "accepted {request}");
    }
    for request in [
        r#"{"op":"connect"}"#,
        r#"{"op":"connect","master":true}"#,
        r#"{"op":"read","serial":"010203040506"}"#,
        r#"{"op":"commit","serial":"010203040506","changes":{"lamp":"1"}}"#,
        r#"{"op":"restore","serial":"010203040506","file":"test.backup.json"}"#,
        r#"{"op":"disconnect"}"#,
        r#"{"op":"backups"}"#,
    ] {
        assert!(parse_request(request).is_ok(), "rejected {request}");
    }
    assert!(matches!(
        parse_request(r#"{"op":"connect"}"#).unwrap(),
        Request::Connect { master: true }
    ));
    assert!(parse_request(&" ".repeat(MAX_REQUEST_BYTES + 1)).is_err());
}

#[test]
fn live_identity_requires_supported_motor_attributes() {
    for (series, unit, params) in [
        (0x02, 0, b"00\0\0\0\0\0".as_slice()), // a Di2 control family
        (0x23, 1, b"00\0\0\0\0\0".as_slice()),
        (0x35, 0, b"00\0\0\0\0\0".as_slice()), // EP801 requires 01
        (0x23, 0, b"unknown".as_slice()),
    ] {
        assert!(motor_support::for_motor_params(series, unit, params).is_err());
    }
    assert_eq!(
        motor_support::for_motor_params(0x35, 0, b"01\0\0\0\0\0")
            .unwrap()
            .settings,
        Some(SettingsModel::Ep801)
    );
    assert!(UnitIdentity::from_reply(13, vec![13, 2, 0])
        .unwrap()
        .require_motor()
        .is_err());
}

#[test]
fn identity_change_invalidates_earlier_staged_values() {
    let expected = motor();
    for changed in [
        Motor {
            serial: "111213141516".into(),
            ..expected.clone()
        },
        Motor {
            model: SettingsModel::Ep801,
            ..expected.clone()
        },
        Motor {
            params: b"00-CRG\0".to_vec(),
            ..expected.clone()
        },
        Motor {
            dcas_version: "4.2.0.0".into(),
            ..expected.clone()
        },
        Motor {
            slot: 14,
            ..expected.clone()
        },
    ] {
        assert!(expected.check_same(&changed).is_err());
    }
    assert!(expected.check_same(&expected).is_ok());
    let reassigned = Motor {
        slot: 14,
        ..expected.clone()
    };
    assert!(expected.check_same(&reassigned).is_err());
    let replaced = Motor {
        slot: 14,
        serial: "111213141516".into(),
        ..expected.clone()
    };
    assert!(expected.check_same(&replaced).is_err());
}

#[test]
fn every_staged_value_must_pass_the_live_motor_schema() {
    let schema = settings::ebike_schema_for_model(SettingsModel::Ep800Crg);
    assert!(preflight_changes(&schema, &BTreeMap::from([("lamp".into(), "1".into())])).is_ok());
    assert!(preflight_changes(
        &schema,
        &BTreeMap::from([
            ("lamp".into(), "1".into()),
            ("tire-circumference".into(), "99999".into()),
        ])
    )
    .is_err());
    assert!(preflight_changes(
        &schema,
        &BTreeMap::from([("derailleur-mode".into(), "1".into())])
    )
    .is_err());
    assert!(preflight_changes(&schema, &BTreeMap::new()).is_err());
    assert!(settings::ebike_schema_for_model(SettingsModel::Component("ST-6770-L")).is_empty());
}

#[test]
fn tuning_snapshot_never_queries_transmission_groups() {
    #[derive(Default)]
    struct Queries(Vec<(u8, u8)>);
    impl UnitCommands for Queries {
        fn command(
            &mut self,
            _slot: u8,
            group: u8,
            command: u8,
            _params: &[u8],
            _response: u8,
        ) -> Result<Vec<u8>> {
            self.0.push((group, command));
            bail!("fixture has no value")
        }
    }
    for model in all_motor_models() {
        let mut queries = Queries::default();
        let snapshot = settings::read_ebike_for_model(&mut queries, 13, model);
        assert!(!queries.0.is_empty());
        assert!(queries
            .0
            .iter()
            .all(|(group, _)| !matches!(*group, 0x0a | 0x0b)));
        assert_eq!(
            snapshot.unavailable.len(),
            settings::ebike_schema_for_model(model).len()
        );
    }
}

fn all_motor_models() -> [SettingsModel; 12] {
    [
        SettingsModel::Ep800Crg,
        SettingsModel::Ep600,
        SettingsModel::Ep801,
        SettingsModel::Ep500,
        SettingsModel::E5100,
        SettingsModel::E8000,
        SettingsModel::E7000,
        SettingsModel::E6100,
        SettingsModel::E6000,
        SettingsModel::E6001,
        SettingsModel::E6002Pre33,
        SettingsModel::E6002,
    ]
}

#[test]
fn every_motor_family_excludes_di2_and_drivetrain_fields() {
    let excluded = [
        "shifting-advice",
        "auto-gear-mode",
        "rear-teeth-pattern",
        "auto-gear-cadence",
        "shifting-system",
        "rear-max-gear",
        "front-teeth",
        "rear-teeth",
        "start-mode",
        "shift-limit-up",
        "shift-limit-down",
    ];
    for model in all_motor_models() {
        let schema = settings::ebike_schema_for_model(model);
        assert!(!schema.is_empty());
        assert!(schema.iter().any(|s| s.name == "lamp"));
        assert!(schema.iter().any(|s| s.name == "tire-circumference"));
        for name in excluded {
            assert!(
                spec(&schema, name).is_err(),
                "exposed {name} on {}",
                model.name()
            );
            assert!(
                preflight_changes(&schema, &BTreeMap::from([(name.into(), "1".into())])).is_err()
            );
        }
        if matches!(
            model,
            SettingsModel::E6000
                | SettingsModel::E6001
                | SettingsModel::E6002Pre33
                | SettingsModel::E6100
                | SettingsModel::E7000
                | SettingsModel::E8000
        ) {
            assert_eq!(schema.len(), 3);
        }
        if model == SettingsModel::E6002 {
            assert_eq!(schema.len(), 4);
            assert_eq!(spec(&schema, "destination").unwrap().choices, [0, 1, 4]);
        }
        if modern(model) {
            for name in [
                "light-output",
                "torque-minimum",
                "torque-maximum",
                "max-power.boost",
            ] {
                assert!(spec(&schema, name).is_err());
            }
            assert!(spec(&schema, "profile1.trail.character").is_ok());
        }
    }
}

#[test]
fn e6002_schema_tracks_live_firmware_and_supported_stock_attributes() {
    for params in [[0, 0x20, 0, 0, 0, 0, 0], [1, 0x20, 0, 0, 0, 0, 0]] {
        for (firmware, expected) in [
            ("3.2.99.0", SettingsModel::E6002Pre33),
            ("3.3.0.0", SettingsModel::E6002),
            ("3.4.0.0", SettingsModel::E6002),
        ] {
            let model = motor_support::settings_for_motor(0x15, 0, &params, firmware).unwrap();
            assert_eq!(model, expected);
            assert_eq!(
                spec(&settings::ebike_schema_for_model(model), "destination").is_ok(),
                model == SettingsModel::E6002
            );
        }
    }
    for (series, params, model) in [
        (0x08, &[][..], SettingsModel::E6000),
        (0x0a, &[][..], SettingsModel::E6001),
        (0x0c, &[0, 0x10, 0, 0, 0, 0, 0][..], SettingsModel::E6001),
    ] {
        assert_eq!(
            motor_support::settings_for_motor(series, 0, params, "3.4.0.0").unwrap(),
            model
        );
    }
    assert!(motor_support::settings_for_motor(0x15, 0, &[0; 7], "3.3.0.0").is_err());
    assert!(motor_support::settings_for_motor(0x15, 0, &[0, 0x20, 0, 0, 0, 0, 0], "3.3").is_err());
}

#[test]
fn modern_torque_constraints_are_read_only_live_evidence_without_guessed_levels() {
    struct Bounds {
        reply: Option<Vec<u8>>,
        calls: usize,
    }
    impl UnitCommands for Bounds {
        fn command(
            &mut self,
            slot: u8,
            group: u8,
            command: u8,
            params: &[u8],
            response: u8,
        ) -> Result<Vec<u8>> {
            assert_eq!((slot, group, command, response), (13, 0x17, 0x1c, 0x1e));
            assert_eq!(params, [0]);
            self.calls += 1;
            self.reply.clone().context("OEM bounds unavailable")
        }
    }
    for model in [
        SettingsModel::Ep600,
        SettingsModel::Ep801,
        SettingsModel::Ep500,
    ] {
        for reply in [vec![20, 85], vec![20, 50, 0xff, 0xff], vec![25, 65]] {
            let mut bounds = Bounds {
                reply: Some(reply.clone()),
                calls: 0,
            };
            assert_eq!(
                torque_constraints(&mut bounds, 13, model),
                Some(json!({
                    "torque_minimum":reply[0], "torque_maximum":reply[1],
                }))
            );
            assert_eq!(bounds.calls, 1);
        }
        for reply in [None, Some(vec![]), Some(vec![20])] {
            let mut bounds = Bounds { reply, calls: 0 };
            assert_eq!(torque_constraints(&mut bounds, 13, model), None);
            assert_eq!(bounds.calls, 1);
        }
    }
    for model in all_motor_models()
        .into_iter()
        .filter(|model| !modern(*model))
    {
        let mut bounds = Bounds {
            reply: Some(vec![20, 85]),
            calls: 0,
        };
        assert_eq!(torque_constraints(&mut bounds, 13, model), None);
        assert_eq!(bounds.calls, 0, "no additional query for {}", model.name());
    }
}

#[test]
fn date_and_name_backups_bind_model_and_serial_before_each_write() {
    struct Editable {
        name: [u8; 10],
        date: [u8; 4],
        backup: PathBuf,
        model: SettingsModel,
    }
    impl UnitCommands for Editable {
        fn command(
            &mut self,
            _slot: u8,
            group: u8,
            command: u8,
            params: &[u8],
            _reply: u8,
        ) -> Result<Vec<u8>> {
            match (group, command) {
                (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                (0x48, 0x54) => Ok(self.date.to_vec()),
                (0x17, 0x24) => {
                    let start = usize::from(params[0] & 1) * 5;
                    Ok([vec![params[0]], self.name[start..start + 5].to_vec()].concat())
                }
                (0x48, 0x50) | (0x17, 0x20) => {
                    // This callback is the first opportunity to send a SET.
                    // Its original and identity must already be on disk.
                    let backup: Value = serde_json::from_slice(&std::fs::read(&self.backup)?)?;
                    assert_eq!(backup["settings_model"], self.model.slug());
                    assert_eq!(backup["serial"], "010203040506");
                    assert!(backup["original_parameters"].as_str().is_some());
                    if group == 0x48 {
                        self.date.copy_from_slice(&params[..4]);
                    } else {
                        let start = usize::from(params[0] & 1) * 5;
                        self.name[start..start + 5].copy_from_slice(&params[1..6]);
                    }
                    Ok(Vec::new())
                }
                _ => panic!("unexpected command {group:02x}/{command:02x}"),
            }
        }
    }
    let directory = std::env::temp_dir().join(format!(
        "cranklink-backup-model-test-{}-{}",
        std::process::id(),
        BACKUP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    for model in [SettingsModel::Ep800Crg, SettingsModel::Ep801] {
        for (name, value) in [
            ("maintenance-date", "2025-06-07"),
            ("profile1.name", "NewName"),
        ] {
            let backup = directory.join(format!("{}-{name}.backup.json", model.slug()));
            let mut editable = Editable {
                name: *b"BeforeName",
                date: [24, 1, 2, 0],
                backup: backup.clone(),
                model,
            };
            let receipt =
                settings::set_for_model(&mut editable, 13, name, value, &backup, model).unwrap();
            assert!(receipt.changed);
            let parsed = settings::parse_restore_file(&backup).unwrap();
            let identity = Motor { model, ..motor() };
            preflight_restore(&parsed, &identity, &settings::ebike_schema_for_model(model))
                .unwrap();
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn restore_is_bound_to_serial_model_and_the_exposed_tuning_schema() {
    let motor = motor();
    let schema = settings::ebike_schema_for_model(motor.model);
    let valid = ParsedRestore {
        serial: motor.serial.clone(),
        setting: "lamp".into(),
        before: json!(1),
        original_parameters: vec![1],
        raw_backup: json!({"settings_model":"ep800-crg"}),
    };
    assert!(preflight_restore(&valid, &motor, &schema).is_ok());
    let mut other = valid.clone();
    other.serial = "111213141516".into();
    assert!(preflight_restore(&other, &motor, &schema).is_err());
    other = valid.clone();
    other.raw_backup["settings_model"] = json!("ep801");
    assert!(preflight_restore(&other, &motor, &schema).is_err());
    other = valid.clone();
    other.raw_backup = json!({});
    assert!(preflight_restore(&other, &motor, &schema).is_err());
    other = valid.clone();
    other.setting = "shift-mode".into();
    assert!(preflight_restore(&other, &motor, &schema).is_err());
}

#[test]
fn private_backup_restore_rejects_paths_journals_and_symlinks() {
    for name in [
        "../a.backup.json",
        "/a.backup.json",
        "a/b.backup.json",
        "a\\b.backup.json",
        ".backup.json",
        "a.journal.jsonl",
    ] {
        assert!(!valid_backup_name(name), "accepted {name}");
    }
    let directory = std::env::temp_dir().join(format!(
        "cranklink-android-test-{}-{}",
        std::process::id(),
        BACKUP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = std::fs::canonicalize(directory).unwrap();
    std::fs::write(directory.join("first.backup.json"), b"{}").unwrap();
    std::fs::write(directory.join("ignored.journal.jsonl"), b"{}").unwrap();
    assert!(restore_path(&directory, "first.backup.json").is_ok());
    assert_eq!(list_backups(&directory).unwrap(), vec!["first.backup.json"]);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            directory.join("first.backup.json"),
            directory.join("link.backup.json"),
        )
        .unwrap();
        assert!(restore_path(&directory, "link.backup.json").is_err());
        assert_eq!(list_backups(&directory).unwrap(), vec!["first.backup.json"]);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
