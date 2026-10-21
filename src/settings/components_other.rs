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
    fn component_writes_journal_then_read_back_exact_shared_tuples() {
        use crate::device::UnitCommands;
        use crate::settings::{set_for_model, SettingsModel};

        struct Component {
            get: u8,
            set: u8,
            current: [u8; 2],
            backup: std::path::PathBuf,
            expected: [u8; 4],
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
                    (0x0b, command) if command == self.get => {
                        assert_eq!(params, [0]);
                        assert_eq!(reply, command + 2);
                        Ok(self.current.to_vec())
                    }
                    (0x0b, command) if command == self.set => {
                        assert!(self.backup.exists());
                        assert_eq!(params, self.expected);
                        assert_eq!(reply, command + 2);
                        self.current.copy_from_slice(&params[..2]);
                        self.writes += 1;
                        Ok(Vec::new())
                    }
                    _ => panic!("unexpected component command {group:02x}/{command:02x}"),
                }
            }
        }

        for (index, model, name, value, get, set, current, expected) in [
            (
                0,
                "SM-BMR2",
                "multi-shift-limit.up",
                "2",
                0x64,
                0x60,
                [255, 3],
                [2, 3, 0, 0],
            ),
            (
                1,
                "MU-S705",
                "hub-adjustment",
                "-3",
                0x14,
                0x10,
                [0, 4],
                [253, 4, 0, 0],
            ),
            (
                2,
                "MU-UR500",
                "hub-gear-count",
                "5",
                0x1c,
                0x18,
                [1, 8],
                [1, 5, 0, 0],
            ),
        ] {
            let backup = std::env::temp_dir().join(format!(
                "cranklink-component-setting-{}-{}-{index}.json",
                std::process::id(),
                crate::platform::now_ms()
            ));
            let mut component = Component {
                get,
                set,
                current,
                backup: backup.clone(),
                expected,
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
            assert_eq!(journal["original_parameters"], hex::encode(current));
            assert_eq!(journal["planned_parameters"], hex::encode(expected));
            std::fs::remove_file(backup).unwrap();
        }
    }
}
