//! Model-gated Di2 power, internal-hub, and display service settings.
//!
//! Wire evidence: vendor SmBmr1Unit.Get/SetLumpShiftLimit,
//! MUUnitDataLink.Get/SetAdjust and Get/SetGearMinMax, SCUnitDataLink.
//! Model/range evidence: Android DI2MasterUnit, GearNumberLimit,
//! MUS705Unit/MUUR500Unit/MUUR510Unit/MUUR520Unit, and each SC class's
//! canCustomize flags and displaymonitor enums. Catalog identity is not a
//! substitute for those per-model capability declarations.

use super::SettingSpec;
use anyhow::{ensure, Result};

pub(super) fn schema(name: &str) -> Vec<SettingSpec> {
    let mut specs = Vec::new();
    if matches!(name, "SM-BMR1" | "SM-BMR2" | "BM-DN100" | "BT-DN110") {
        specs.extend(shift_limits());
    }
    match name {
        "MU-S705" | "MU-UR500" | "MU-UR510" | "MU-UR520" => {
            let mut adjustment = SettingSpec::new(
                "hub-adjustment",
                "adjustment steps",
                (-4, 4, 1),
                0x0b,
                0x10,
                &[0],
                &[0; 4],
                0,
                1,
            );
            adjustment.signed = true;
            adjustment.preserve = Some(2);
            specs.push(adjustment);

            // UR520 has no selectable maximum shift-count list. Its catalog
            // membership must not accidentally enable a gearbox-count write.
            if name != "MU-UR520" {
                let choices: &[i64] = if name == "MU-S705" {
                    &[8, 11]
                } else {
                    &[5, 8, 11]
                };
                let mut gears = SettingSpec::new(
                    "hub-gear-count",
                    "gears",
                    (choices[0], 11, 1),
                    0x0b,
                    0x18,
                    &[0],
                    &[0; 4],
                    1,
                    1,
                )
                .choices(choices);
                gears.preserve = Some(2);
                specs.push(gears);
            }
        }
        _ => {}
    }

    if matches!(
        name,
        "MU-S705" | "MU-UR500" | "MU-UR510" | "MU-UR520" | "FH-U6060"
    ) {
        let mut start_mode = SettingSpec::new(
            "start-mode",
            "gear, 0=off",
            (0, 11, 1),
            0x0b,
            0x88,
            &[0],
            &[0; 4],
            0,
            1,
        );
        start_mode.preserve = Some(2);
        specs.push(start_mode);

        let mut cadence = SettingSpec::new(
            "auto-gear-cadence",
            "rpm",
            (500, 760, 10),
            0x0b,
            0x90,
            &[0],
            &[0; 4],
            0,
            2,
        );
        cadence.scale = 10.0;
        cadence.preserve = Some(2);
        specs.push(cadence);
    }

    // The Android model classes explicitly enable these controls. Newer
    // displays with different or unspecified flags stay outside each list.
    if matches!(
        name,
        "SC-M9050" | "SC-M9051" | "SC-E8000" | "SC-E6100" | "SC-E7000" | "SC-EM800" | "SC-EN610"
    ) {
        specs.push(byte_setting(
            "display-beep",
            "0 off, 1 on",
            0x23,
            0,
            &[0, 1],
        ));
    }
    if matches!(
        name,
        "SC-E8000" | "SC-E6100" | "SC-E7000" | "SC-EM800" | "SC-EN500" | "SC-EN600" | "SC-EN610"
    ) {
        specs.push(byte_setting(
            "display-distance-unit",
            "0 kilometres, 1 miles",
            0x05,
            0x18,
            &[0, 1],
        ));
    }
    if matches!(name, "SC-E6100" | "SC-EN500") {
        specs.push(byte_setting(
            "display-backlight",
            "0 off, 1 on, 2 manual",
            0x05,
            0,
            &[0, 1, 2],
        ));
    }
    if matches!(
        name,
        "SC-E8000" | "SC-E6100" | "SC-E7000" | "SC-EM800" | "SC-EN600" | "SC-EN610"
    ) {
        specs.push(byte_setting(
            "display-language",
            "0 English, 1 French, 2 German, 3 Dutch, 4 Italian, 5 Spanish",
            0x05,
            0x20,
            &[0, 1, 2, 3, 4, 5],
        ));
    }
    if matches!(name, "SC-E8000" | "SC-EM800" | "SC-EN600" | "SC-EN610") {
        // GET reports one meaningful byte; vendor SET uses a four-byte
        // little-endian tuple whose remaining bytes are zero, not GET data.
        let mut brightness = SettingSpec::new(
            "display-brightness",
            "percent",
            (20, 100, 20),
            0x05,
            0x50,
            &[0],
            &[0; 4],
            0,
            1,
        )
        .choices(&[20, 40, 60, 80, 100]);
        brightness.preserve = Some(1);
        specs.push(brightness);
    }
    if matches!(
        name,
        "SC-E8000" | "SC-E6100" | "SC-E7000" | "SC-EM800" | "SC-EN600" | "SC-EN610"
    ) {
        let mut time = SettingSpec::new(
            "display-time",
            "minutes from midnight (0..1439)",
            (0, 1439, 1),
            0x05,
            0x40,
            &[0],
            &[0; 2],
            0,
            2,
        );
        time.preserve = Some(2);
        specs.push(time);
    }
    if matches!(
        name,
        "EW-WU101"
            | "EW-WU111"
            | "EW-EN100"
            | "SC-E8000"
            | "SC-E6100"
            | "SC-E7000"
            | "SC-EM800"
            | "SC-EN600"
            | "SC-EN610"
    ) {
        specs.push(byte_setting(
            "wireless-mode",
            "0 BLE and ANT+, 1 ANT+ only",
            0x40,
            0x50,
            &[0, 1],
        ));
        let mut passkey = SettingSpec::new(
            "bluetooth-passkey",
            "6-digit numeric PIN",
            (0, 0, 1),
            0x40,
            0x08,
            &[0],
            &[0; 6],
            0,
            6,
        );
        passkey.value_type = "string".into();
        passkey.preserve = Some(6);
        specs.push(passkey);
    }
    specs
}

fn byte_setting(name: &str, unit: &str, group: u8, set: u8, choices: &[i64]) -> SettingSpec {
    let mut spec = SettingSpec::new(
        name,
        unit,
        (choices[0], choices[choices.len() - 1], 1),
        group,
        set,
        &[0],
        &[0],
        0,
        1,
    )
    .choices(choices);
    spec.preserve = Some(1);
    spec
}

fn shift_limits() -> Vec<SettingSpec> {
    ["multi-shift-limit.up", "multi-shift-limit.down"]
        .into_iter()
        .enumerate()
        .map(|(offset, name)| {
            let mut spec = SettingSpec::new(
                name,
                "2 gears, 3 gears, 255 unlimited",
                (2, 255, 1),
                0x0b,
                0x60,
                &[0],
                &[0; 4],
                offset,
                1,
            )
            .choices(&[2, 3, 255]);
            spec.preserve = Some(2);
            spec
        })
        .collect()
}

/// Check the complete meaningful tuple before a sibling-preserving write and
/// again on readback. In particular a hub adjustment must not reuse an unknown
/// actuator limit or mistake a truncated maximum for a valid response.
pub(super) fn validate_reply(spec: &SettingSpec, raw: &[u8]) -> Result<()> {
    match spec.name.as_str() {
        "hub-adjustment" => {
            ensure!(raw.len() >= 2, "short hub-adjustment reply");
            ensure!(
                raw[1] == 4,
                "unsupported hub adjustment maximum: {}",
                raw[1]
            );
            ensure!(
                (-4..=4).contains(&(raw[0] as i8)),
                "hub adjustment is outside the model range"
            );
        }
        "hub-gear-count" => {
            ensure!(raw.len() >= 2, "short hub-gear-count reply");
            ensure!(raw[0] == 1, "unsupported hub minimum gear: {}", raw[0]);
            ensure!(
                spec.choices.contains(&i64::from(raw[1])),
                "hub gear count is outside the model choices"
            );
        }
        "multi-shift-limit.up" | "multi-shift-limit.down" => {
            ensure!(raw.len() >= 2, "short multi-shift-limit reply");
            ensure!(
                raw[..2].iter().all(|n| [2, 3, 255].contains(n)),
                "unsupported multi-shift limit tuple"
            );
        }
        "display-time" => {
            ensure!(raw.len() >= 2, "short display-time reply");
            let minutes = u16::from_le_bytes([raw[0], raw[1]]);
            ensure!(
                minutes < 1440,
                "display time is outside the 24-hour range: {minutes}"
            );
        }
        "wireless-mode" => {
            ensure!(!raw.is_empty(), "short wireless-mode reply");
            ensure!(
                matches!(raw[0], 0 | 1),
                "unsupported wireless mode: {}",
                raw[0]
            );
        }
        "bluetooth-passkey" => {
            ensure!(raw.len() >= 6, "short bluetooth-passkey reply");
            ensure!(
                raw[..6].iter().all(|b| b.is_ascii_digit()),
                "passkey reply contains non-numeric bytes"
            );
        }
        "start-mode" => {
            ensure!(raw.len() >= 2, "short start-mode reply");
            ensure!(raw[0] <= 11, "start mode gear exceeds maximum: {}", raw[0]);
        }
        "auto-gear-cadence" => {
            ensure!(raw.len() >= 2, "short auto-gear-cadence reply");
            let raw_cadence = u16::from_le_bytes([raw[0], raw[1]]);
            ensure!(
                raw_cadence <= 1200,
                "auto-gear-cadence exceeds reasonable range: {raw_cadence}"
            );
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn named(model: &str, name: &str) -> SettingSpec {
        schema(model).into_iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn power_limits_preserve_the_other_direction_and_reject_invalid_codes() {
        let up = named("BT-DN110", "multi-shift-limit.up");
        let down = named("SM-BMR1", "multi-shift-limit.down");
        assert_eq!((up.group, up.get, up.set), (0x0b, 0x64, 0x60));
        assert_eq!(up.query, [0]);
        assert_eq!(
            super::super::updated_payload(&up, &[255, 3], 2, false).unwrap(),
            [2, 3, 0, 0]
        );
        assert_eq!(
            super::super::updated_payload(&down, &[2, 255], 3, false).unwrap(),
            [2, 3, 0, 0]
        );
        assert!(super::super::validate_input(&up, "4").is_err());
        assert!(validate_reply(&up, &[2]).is_err());
        assert!(validate_reply(&up, &[2, 4]).is_err());
        assert!(schema("BT-DN300").is_empty());
    }

    #[test]
    fn hub_adjustment_retains_and_checks_the_device_limit() {
        let spec = named("MU-S705", "hub-adjustment");
        assert_eq!(spec.decode(&[252, 4]).unwrap(), json!(-4));
        assert_eq!(
            super::super::updated_payload(&spec, &[0, 4], -4, false).unwrap(),
            [252, 4, 0, 0]
        );
        assert!(validate_reply(&spec, &[252, 4]).is_ok());
        for reply in [&[0][..], &[0, 3], &[5, 4], &[251, 4]] {
            assert!(validate_reply(&spec, reply).is_err());
        }
        assert!(super::super::validate_input(&spec, "5").is_err());
    }

    #[test]
    fn hub_gear_choices_are_model_specific() {
        let alfine = named("MU-S705", "hub-gear-count");
        let urban = named("MU-UR500", "hub-gear-count");
        assert_eq!(alfine.choices, [8, 11]);
        assert_eq!(urban.choices, [5, 8, 11]);
        assert_eq!((urban.group, urban.get, urban.set), (0x0b, 0x1c, 0x18));
        assert_eq!(
            super::super::updated_payload(&urban, &[1, 8], 5, false).unwrap(),
            [1, 5, 0, 0]
        );
        assert!(validate_reply(&alfine, &[1, 5]).is_err());
        assert!(validate_reply(&urban, &[0, 8]).is_err());
        assert!(schema("MU-UR520")
            .iter()
            .all(|s| s.name != "hub-gear-count"));
    }

    #[test]
    fn hub_start_mode_and_autoshift_cadence() {
        for model in ["MU-S705", "MU-UR500", "MU-UR510", "MU-UR520", "FH-U6060"] {
            let start = named(model, "start-mode");
            assert_eq!((start.group, start.get, start.set), (0x0b, 0x8c, 0x88));
            assert_eq!(start.query, [0]);
            assert_eq!(
                super::super::updated_payload(&start, &[0, 0], 3, false).unwrap(),
                [3, 0, 0, 0]
            );
            assert_eq!(start.decode(&[3, 0]).unwrap(), json!(3));
            assert!(super::super::validate_input(&start, "0").is_ok());
            assert!(super::super::validate_input(&start, "11").is_ok());
            assert!(super::super::validate_input(&start, "12").is_err());
            assert!(validate_reply(&start, &[0, 0]).is_ok());
            assert!(validate_reply(&start, &[11, 0]).is_ok());
            assert!(validate_reply(&start, &[12, 0]).is_err());
            assert!(validate_reply(&start, &[0]).is_err());

            let cadence = named(model, "auto-gear-cadence");
            assert_eq!(
                (cadence.group, cadence.get, cadence.set),
                (0x0b, 0x94, 0x90)
            );
            assert_eq!(cadence.scale, 10.0);
            assert_eq!(
                super::super::updated_payload(&cadence, &[0, 0], 700, false).unwrap(),
                [0xbc, 0x02, 0, 0]
            );
            assert_eq!(cadence.decode(&[0xbc, 0x02]).unwrap(), json!(70.0));
            assert!(super::super::validate_input(&cadence, "50").is_ok());
            assert!(super::super::validate_input(&cadence, "76").is_ok());
            assert!(super::super::validate_input(&cadence, "49").is_err());
            assert!(super::super::validate_input(&cadence, "77").is_err());
            assert!(validate_reply(&cadence, &[0xbc, 0x02]).is_ok());
            assert!(validate_reply(&cadence, &[0, 0]).is_ok());
            assert!(validate_reply(&cadence, &[0xff, 0xff]).is_err());
            assert!(validate_reply(&cadence, &[0]).is_err());
        }

        assert!(schema("FH-U6060")
            .iter()
            .all(|s| s.name != "hub-adjustment" && s.name != "hub-gear-count"));
    }

    #[test]
    fn display_controls_follow_per_model_flags_and_exact_payload_widths() {
        let language = named("SC-E8000", "display-language");
        assert_eq!(language.preserve, Some(1));
        assert_eq!(
            super::super::updated_payload(&language, &[0], 2, false).unwrap(),
            [2]
        );
        let brightness = named("SC-E8000", "display-brightness");
        assert_eq!(
            (brightness.group, brightness.get, brightness.set),
            (5, 0x54, 0x50)
        );
        assert_eq!(brightness.decode(&[60]).unwrap(), json!(60));
        assert_eq!(
            super::super::updated_payload(&brightness, &[60], 100, false).unwrap(),
            [100, 0, 0, 0]
        );
        assert!(super::super::validate_input(&brightness, "50").is_err());
        assert_eq!(schema("SC-M9050").len(), 1);
        assert!(schema("SC-EN500").iter().all(|s| matches!(
            s.name.as_str(),
            "display-backlight" | "display-distance-unit"
        )));
        assert!(schema("SC-E7000")
            .iter()
            .all(|s| s.name != "display-brightness"));
        assert!(schema("BT-E8010").is_empty());
    }

    #[test]
    fn wireless_mode_and_passkey_support_gateways_and_wireless_displays() {
        for gateway in ["EW-WU101", "EW-WU111", "EW-EN100"] {
            let wm = named(gateway, "wireless-mode");
            assert_eq!((wm.group, wm.get, wm.set), (0x40, 0x54, 0x50));
            assert_eq!(wm.choices, [0, 1]);
            assert_eq!(
                super::super::updated_payload(&wm, &[0], 1, false).unwrap(),
                [1]
            );
            assert!(validate_reply(&wm, &[0]).is_ok());
            assert!(validate_reply(&wm, &[1]).is_ok());
            assert!(validate_reply(&wm, &[2]).is_err());
            assert!(validate_reply(&wm, &[]).is_err());

            let passkey = named(gateway, "bluetooth-passkey");
            assert_eq!(
                (passkey.group, passkey.get, passkey.set),
                (0x40, 0x0c, 0x08)
            );
            assert_eq!(passkey.value_type, "string");
            assert_eq!(passkey.decode(b"123456").unwrap(), json!("123456"));
            assert!(super::super::validate_input(&passkey, "123456").is_ok());
            assert!(super::super::validate_input(&passkey, "876543").is_ok());
            assert!(super::super::validate_input(&passkey, "000000").is_ok());
            assert!(super::super::validate_input(&passkey, "999999").is_ok());
            assert!(super::super::validate_input(&passkey, "12345").is_err());
            assert!(super::super::validate_input(&passkey, "1234567").is_err());
            assert!(super::super::validate_input(&passkey, "12345a").is_err());
            assert!(validate_reply(&passkey, b"123456").is_ok());
            assert!(validate_reply(&passkey, b"000000").is_ok());
            assert!(validate_reply(&passkey, b"999999").is_ok());
            assert!(validate_reply(&passkey, b"12345").is_err());
            assert!(validate_reply(&passkey, b"12345a").is_err());
        }

        for display in [
            "SC-E8000", "SC-E6100", "SC-E7000", "SC-EM800", "SC-EN600", "SC-EN610",
        ] {
            assert!(schema(display).iter().any(|s| s.name == "wireless-mode"));
            assert!(schema(display)
                .iter()
                .any(|s| s.name == "bluetooth-passkey"));
        }

        for wired in ["SC-EN500", "SC-M9050", "MU-S705", "SM-BMR1"] {
            assert!(schema(wired)
                .iter()
                .all(|s| s.name != "wireless-mode" && s.name != "bluetooth-passkey"));
        }
    }

    #[test]
    fn display_time_is_unsigned_little_endian_minutes_from_midnight() {
        for model in [
            "SC-E8000", "SC-E6100", "SC-E7000", "SC-EM800", "SC-EN600", "SC-EN610",
        ] {
            let time = named(model, "display-time");
            assert_eq!((time.group, time.get, time.set), (0x05, 0x44, 0x40));
            assert_eq!(time.query, [0]);
            assert_eq!(time.decode(&[0x15, 0x04]).unwrap(), json!(1045));
            assert_eq!(time.decode(&[0x00, 0x00]).unwrap(), json!(0));
            assert_eq!(time.decode(&[0x9f, 0x05]).unwrap(), json!(1439));
            assert_eq!(
                super::super::updated_payload(&time, &[0, 0], 1045, false).unwrap(),
                [0x15, 0x04]
            );
            assert!(super::super::validate_input(&time, "0").is_ok());
            assert!(super::super::validate_input(&time, "1045").is_ok());
            assert!(super::super::validate_input(&time, "1439").is_ok());
            assert!(super::super::validate_input(&time, "1440").is_err());
            assert!(super::super::validate_input(&time, "-1").is_err());
            assert!(super::super::validate_input(&time, "abc").is_err());
            assert!(validate_reply(&time, &[0x15, 0x04]).is_ok());
            assert!(validate_reply(&time, &[0x9f, 0x05]).is_ok());
            assert!(validate_reply(&time, &[0xa0, 0x05]).is_err());
            assert!(validate_reply(&time, &[0]).is_err());
        }

        for model in ["EW-WU111", "SC-EN500", "SC-M9050", "MU-S705"] {
            assert!(schema(model).iter().all(|s| s.name != "display-time"));
        }
    }

    #[test]
    fn component_writes_journal_then_read_back_exact_shared_tuples() {
        use crate::device::UnitCommands;
        use crate::settings::{set_for_model, SettingsModel};

        struct Component {
            group: u8,
            get: u8,
            set: u8,
            current: Vec<u8>,
            backup: std::path::PathBuf,
            expected: Vec<u8>,
            writes: usize,
        }
        impl UnitCommands for Component {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                reply: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!(slot, 7);
                match (group, command) {
                    (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                    (grp, command) if grp == self.group && command == self.get => {
                        assert_eq!(params, [0]);
                        assert_eq!(reply, command + 2);
                        Ok(self.current.clone())
                    }
                    (grp, command) if grp == self.group && command == self.set => {
                        assert!(self.backup.exists());
                        assert_eq!(params, self.expected.as_slice());
                        assert_eq!(reply, command + 2);
                        self.current = params.to_vec();
                        self.writes += 1;
                        Ok(Vec::new())
                    }
                    _ => panic!("unexpected component command {group:02x}/{command:02x}"),
                }
            }
        }

        for (index, model, name, value, group, get, set, current, expected) in [
            (
                0,
                "SM-BMR2",
                "multi-shift-limit.up",
                "2",
                0x0b,
                0x64,
                0x60,
                vec![255, 3],
                vec![2, 3, 0, 0],
            ),
            (
                1,
                "MU-S705",
                "hub-adjustment",
                "-3",
                0x0b,
                0x14,
                0x10,
                vec![0, 4],
                vec![253, 4, 0, 0],
            ),
            (
                2,
                "MU-UR500",
                "hub-gear-count",
                "5",
                0x0b,
                0x1c,
                0x18,
                vec![1, 8],
                vec![1, 5, 0, 0],
            ),
            (
                3,
                "EW-WU111",
                "wireless-mode",
                "1",
                0x40,
                0x54,
                0x50,
                vec![0],
                vec![1],
            ),
            (
                4,
                "SC-E8000",
                "display-time",
                "1045",
                0x05,
                0x44,
                0x40,
                vec![0, 0],
                vec![0x15, 0x04],
            ),
            (
                5,
                "EW-EN100",
                "bluetooth-passkey",
                "876543",
                0x40,
                0x0c,
                0x08,
                b"123456".to_vec(),
                b"876543".to_vec(),
            ),
            (
                6,
                "MU-UR510",
                "start-mode",
                "3",
                0x0b,
                0x8c,
                0x88,
                vec![0, 0],
                vec![3, 0, 0, 0],
            ),
            (
                7,
                "FH-U6060",
                "auto-gear-cadence",
                "75",
                0x0b,
                0x94,
                0x90,
                vec![0xbc, 0x02],
                vec![0xee, 0x02, 0, 0],
            ),
        ] {
            let backup = std::env::temp_dir().join(format!(
                "cranklink-component-setting-{}-{}-{index}.json",
                std::process::id(),
                crate::platform::now_ms()
            ));
            let mut component = Component {
                group,
                get,
                set,
                current: current.clone(),
                backup: backup.clone(),
                expected: expected.clone(),
                writes: 0,
            };
            let receipt = set_for_model(
                &mut component,
                7,
                name,
                value,
                &backup,
                SettingsModel::Component(model),
            )
            .unwrap();
            assert!(receipt.changed);
            assert_eq!(component.writes, 1);
            let journal: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
            assert_eq!(journal["original_parameters"], hex::encode(&current));
            assert_eq!(journal["planned_parameters"], hex::encode(&expected));
            std::fs::remove_file(backup).unwrap();
        }
    }

    #[test]
    fn dry_run_component_settings() {
        use crate::device::UnitCommands;
        use crate::settings::{dry_run_for_model, SettingsModel};

        struct MockGateway {
            passkey: Vec<u8>,
        }
        impl UnitCommands for MockGateway {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                reply: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!(slot, 2);
                match (group, command) {
                    (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                    (0x40, 0x0c) => {
                        assert_eq!(params, [0]);
                        assert_eq!(reply, 0x0e);
                        Ok(self.passkey.clone())
                    }
                    _ => panic!("unexpected command {group:02x}/{command:02x}"),
                }
            }
        }

        let mut gw = MockGateway {
            passkey: b"000000".to_vec(),
        };
        let report = dry_run_for_model(
            &mut gw,
            2,
            "bluetooth-passkey",
            "876543",
            SettingsModel::Component("EW-EN100"),
        )
        .unwrap();
        assert_eq!(report["valid"], true);
        assert_eq!(report["changed"], true);
        assert_eq!(report["current_value"], "000000");
        assert_eq!(report["planned_value"], "876543");
        assert_eq!(report["planned_parameters"], hex::encode("876543"));

        assert!(dry_run_for_model(
            &mut gw,
            2,
            "bluetooth-passkey",
            "000000",
            SettingsModel::Component("EW-EN100"),
        )
        .is_err());
        assert!(dry_run_for_model(
            &mut gw,
            2,
            "bluetooth-passkey",
            "999999",
            SettingsModel::Component("EW-EN100"),
        )
        .is_err());

        struct MockHub {
            start_mode: Vec<u8>,
            cadence: Vec<u8>,
        }
        impl UnitCommands for MockHub {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                reply: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!(slot, 3);
                match (group, command) {
                    (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                    (0x0b, 0x8c) => {
                        assert_eq!(params, [0]);
                        assert_eq!(reply, 0x8e);
                        Ok(self.start_mode.clone())
                    }
                    (0x0b, 0x94) => {
                        assert_eq!(params, [0]);
                        assert_eq!(reply, 0x96);
                        Ok(self.cadence.clone())
                    }
                    _ => panic!("unexpected hub command {group:02x}/{command:02x}"),
                }
            }
        }

        let mut hub = MockHub {
            start_mode: vec![0, 0],
            cadence: vec![0xbc, 0x02], // 700 = 70.0 rpm
        };
        let start_report = dry_run_for_model(
            &mut hub,
            3,
            "start-mode",
            "3",
            SettingsModel::Component("MU-UR500"),
        )
        .unwrap();
        assert_eq!(start_report["valid"], true);
        assert_eq!(start_report["changed"], true);
        assert_eq!(start_report["current_value"], 0);
        assert_eq!(start_report["planned_value"], "3");
        assert_eq!(
            start_report["planned_parameters"],
            hex::encode([3, 0, 0, 0])
        );

        let cadence_report = dry_run_for_model(
            &mut hub,
            3,
            "auto-gear-cadence",
            "75",
            SettingsModel::Component("FH-U6060"),
        )
        .unwrap();
        assert_eq!(cadence_report["valid"], true);
        assert_eq!(cadence_report["changed"], true);
        assert_eq!(cadence_report["current_value"], 70.0);
        assert_eq!(cadence_report["planned_value"], "75");
        assert_eq!(
            cadence_report["planned_parameters"],
            hex::encode([0xee, 0x02, 0, 0])
        );
    }
}
