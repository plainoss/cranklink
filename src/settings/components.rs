//! Verified component service settings: switch assignments, derailleur
//! adjustment, multi-shift speed and synchronized-shift interval. Identity
//! catalog entries alone never opt a product into one of these wire layouts.
//! Modern wireless switch routing and extended function codes are deliberately
//! separate from the classic wired switch dictionary below.

use super::SettingSpec;
use anyhow::{ensure, Result};

pub(super) fn validate_reply(spec: &SettingSpec, raw: &[u8]) -> Result<()> {
    if spec.group == 4 && spec.set == 0x10 && spec.query.len() == 4 {
        ensure!(raw.len() >= 2, "short classic switch condition reply");
    }
    Ok(())
}

pub(super) fn schema(name: &str) -> Vec<SettingSpec> {
    let mut specs = match name {
        "ST-6770-L" | "ST-6770-R" | "SW-R600" | "SW-R671-L" | "SW-R671-R" | "ST-9071-L"
        | "ST-9071-R" | "SW-9071-L" | "SW-9071-R" | "ST-6871-L" | "ST-6871-R" | "SW-S705" => {
            switch_schema(false)
        }
        "ST-9070-L" | "ST-9070-R" | "ST-6870-L" | "ST-6870-R" | "ST-R785-L" | "ST-R785-R" => {
            switch_schema(true)
        }
        "ST-R9150-L" | "ST-R9150-R" | "ST-R9170-L" | "ST-R9170-R" | "ST-R8050-L" | "ST-R8050-R"
        | "ST-R8070-L" | "ST-R8070-R" | "ST-RX815-L" | "ST-RX815-R" | "ST-R9171-L"
        | "ST-R9171-R" | "ST-R8071-L" | "ST-R8071-R" => extended_switch_schema(),
        // Wired 12-speed levers use six-bit codes with a third byte of high
        // bits, and wireless 12-speed levers are configured through another
        // unit; neither fits this four-bit layout. EN600/EM800 STEPS switches
        // share the six-bit layout.
        "SW-E7000-L" | "SW-E6000" | "SW-E6010-L" => steps_switch_schema(
            STEPS_SHIFT_AND_ASSIST,
            match name {
                "SW-E6000" => Some("switch-c-function"),
                "SW-E6010-L" => Some("switch-s-function"),
                _ => None,
            },
        ),
        "SW-E7000-R" => steps_switch_schema(STEPS_SHIFT, None),
        "SW-E8000-L" => steps_switch_schema(STEPS_ASSIST, None),
        _ => Vec::new(),
    };
    // S705's ordinary Di2/single-system S4 assignment pattern contains rear
    // shifting only. Front assignments require a separately established
    // D-FLY-capable combination, which this schema does not infer.
    if name == "SW-S705" {
        for spec in &mut specs {
            spec.min = 2;
            spec.max = 3;
            spec.choices = vec![2, 3];
        }
    }

    match name {
        "RD-6770" | "RD-9070" | "RD-6870-GS" | "RD-6870-SS" | "RD-M9050-GS" | "RD-M9050-SGS"
        | "RD-M8050-GS" | "RD-R9150" | "RD-R8050-GS" | "RD-R8050-SS" | "RD-RX805-GS"
        | "RD-RX815" | "RD-RX817" | "RD-R9250" | "RD-R8150" | "RD-R7150" | "RD-RX825"
        | "RD-RX827-SGS" | "RD-RX717-SGS" | "RD-RX828-SGS" => {
            specs.extend(adjustment_schema(0x0b));
            let mut speed = SettingSpec::new(
                "multi-shift-speed",
                "0 off, 1 very slow, 2 slow, 3 normal, 4 fast, 5 very fast, 6 super fast",
                (0, 6, 1),
                0x0b,
                0x50,
                &[0],
                &[0; 4],
                0,
                1,
            )
            .choices(&[0, 1, 2, 3, 4, 5, 6]);
            // A four-byte SET whose byte 0 is the speed; the reserved tail
            // stays zero.
            speed.preserve = Some(1);
            specs.push(speed);
        }
        "FD-6770" | "FD-9070" | "FD-6870" | "FD-M9050" | "FD-M9070" | "FD-M8070" | "FD-R9150"
        | "FD-R8050" | "FD-RX815" | "FD-R9250" | "FD-R8150" | "FD-R7150" | "FD-RX825" => {
            specs.extend(adjustment_schema(0x0a));
            // The first 6770 generation has no synchronized-shift interval.
            if name != "FD-6770" {
                let mut interval = SettingSpec::new(
                    "synchro-shift-interval",
                    "0 normal, 1 slow, 2 very slow",
                    (0, 2, 1),
                    0x0a,
                    0x98,
                    &[0],
                    &[0],
                    0,
                    1,
                )
                .choices(&[0, 1, 2]);
                // The interval SET carries a single byte.
                interval.preserve = Some(1);
                specs.push(interval);
            }
        }
        _ => {}
    }
    specs
}

fn switch_schema(four_byte_layout: bool) -> Vec<SettingSpec> {
    let query = if four_byte_layout {
        vec![0; 4]
    } else {
        vec![0]
    };
    let template = query.clone();
    let mut specs = Vec::new();
    for (name, offset, mask) in [
        ("switch-a-function", 0, 0xf0),
        ("switch-b-function", 0, 0x0f),
        ("switch-c-function", 1, 0xf0),
        ("switch-s-function", 1, 0x0f),
    ] {
        if !four_byte_layout && offset > 0 {
            continue;
        }
        let mut spec = SettingSpec::new(
            name,
            "0 front up, 1 front down, 2 rear up, 3 rear down (wire channel)",
            (0, 3, 1),
            0x04,
            0x10,
            &query,
            &template,
            offset,
            1,
        )
        .choices(&[0, 1, 2, 3]);
        spec.bit_mask = Some(mask);
        // Only the AB/CS bytes of the GET prefix are meaningful and SET bytes
        // 2/3 stay zero. None of the models admitted above report key events;
        // modern six-bit function layouts need a separate schema, rather than
        // treating classic GET padding as events.
        spec.preserve = Some(if four_byte_layout { 2 } else { 1 });
        // C/S include satellite and auxiliary buttons with model-specific
        // assignment restrictions; report their current codes without guessing
        // those restrictions from the primary shift buttons.
        if offset > 0 {
            spec.read_only = true;
            spec.min = 0;
            spec.max = 15;
            spec.choices.clear();
            spec.unit = "wire function code".into();
        }
        specs.push(spec);
    }
    specs
}

fn extended_switch_schema() -> Vec<SettingSpec> {
    let query = vec![0; 4];
    let template = query.clone();
    let mut specs = Vec::new();
    let channels: &[_] = &[
        (
            "switch-a-function",
            0,
            0xf0,
            &[0, 1, 2, 3, 10, 11, 12, 13][..],
            "0 front up, 1 front down, 2 rear up, 3 rear down, 10..13 D-Fly",
        ),
        (
            "switch-b-function",
            0,
            0x0f,
            &[0, 1, 2, 3, 10, 11, 12, 13][..],
            "0 front up, 1 front down, 2 rear up, 3 rear down, 10..13 D-Fly",
        ),
        (
            "switch-c-function",
            1,
            0xf0,
            &[0, 1, 2, 3, 4, 5, 10, 11, 12, 13, 15][..],
            "0 front up, 1 front down, 2 rear up, 3 rear down, 4/5 display, 10..13 D-Fly, 15 disabled",
        ),
        (
            "switch-s-function",
            1,
            0x0f,
            &[0, 1, 2, 3, 4, 5, 10, 11, 12, 13, 15][..],
            "0 front up, 1 front down, 2 rear up, 3 rear down, 4/5 display, 10..13 D-Fly, 15 disabled",
        ),
    ];
    for (name, offset, mask, choices, desc) in channels {
        let mut spec = SettingSpec::new(
            name,
            desc,
            (0, 15, 1),
            0x04,
            0x10,
            &query,
            &template,
            *offset,
            1,
        )
        .choices(choices);
        spec.bit_mask = Some(*mask);
        spec.preserve = Some(2);
        specs.push(spec);
    }
    specs
}

/// STEPS A/B choices per switch pattern. D-Fly codes need a D-Fly drive
/// unit connection this schema cannot confirm, so none are offered.
const STEPS_SHIFT_AND_ASSIST: &[i64] = &[0, 1, 2, 3, 6, 7];
const STEPS_SHIFT: &[i64] = &[0, 1, 2, 3];
const STEPS_ASSIST: &[i64] = &[6, 7];

fn steps_switch_schema(
    primary_choices: &[i64],
    third_button: Option<&'static str>,
) -> Vec<SettingSpec> {
    let query = vec![0; 4];
    let template = query.clone();
    let mut specs = Vec::new();

    // Swapping between shifting and assist only makes sense where both exist.
    if primary_choices == STEPS_SHIFT_AND_ASSIST {
        let mut mode = SettingSpec::new(
            "switch-mode",
            "0 shifting, 1 mixed (read-only), 2 assist; writing replaces the A/B functions",
            (0, 2, 1),
            0x04,
            0x10,
            &query,
            &template,
            0,
            1,
        )
        .choices(&[0, 1, 2]);
        mode.preserve = Some(2);
        specs.push(mode);
    }

    for (name, offset, mask) in [
        ("switch-a-function", 0, 0xf0),
        ("switch-b-function", 0, 0x0f),
    ] {
        let mut spec = SettingSpec::new(
            name,
            "0 front up, 1 front down, 2 rear up, 3 rear down, 6 assist up, 7 assist down",
            (
                primary_choices[0],
                primary_choices[primary_choices.len() - 1],
                1,
            ),
            0x04,
            0x10,
            &query,
            &template,
            offset,
            1,
        )
        .choices(primary_choices);
        spec.bit_mask = Some(mask);
        spec.preserve = Some(2);
        specs.push(spec);
    }

    if let Some(third) = third_button {
        let mask = if third == "switch-c-function" {
            0xf0
        } else {
            0x0f
        };
        let mut spec = SettingSpec::new(
            third,
            "0 front up, 1 front down, 2 rear up, 3 rear down, 4/5 display, 6 assist up, 7 assist down",
            (0, 7, 1),
            0x04,
            0x10,
            &query,
            &template,
            1,
            1,
        )
        .choices(&[0, 1, 2, 3, 4, 5, 6, 7]);
        spec.bit_mask = Some(mask);
        spec.preserve = Some(2);
        specs.push(spec);
    }

    specs
}

fn adjustment_schema(group: u8) -> Vec<SettingSpec> {
    ["derailleur-adjustment", "derailleur-adjustment-maximum"]
        .into_iter()
        .enumerate()
        .map(|(offset, name)| {
            let mut spec = SettingSpec::new(
                name,
                "adjustment steps",
                (-128, 127, 1),
                group,
                0x10,
                &[0],
                &[0; 2],
                offset,
                1,
            );
            // The GET reports current and maximum as signed bytes. Its
            // SET path includes target/mode handling and is not a plain setting.
            spec.signed = true;
            spec.read_only = true;
            spec
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn switch_channels_preserve_siblings_and_zero_reserved_set_bytes() {
        let specs = schema("ST-9070-L");
        let a = &specs[0];
        let b = &specs[1];
        let original = [0x23, 0xaf, 0xff, 0xff];
        assert_eq!(a.decode(&original).unwrap(), json!(2));
        assert_eq!(b.decode(&original).unwrap(), json!(3));
        assert_eq!(specs[2].decode(&original).unwrap(), json!(10));
        assert_eq!(specs[3].decode(&original).unwrap(), json!(15));
        assert_eq!(
            super::super::updated_payload(a, &original, 1, false).unwrap(),
            [0x13, 0xaf, 0, 0]
        );
        assert_eq!(
            super::super::updated_payload(b, &original, 0, false).unwrap(),
            [0x20, 0xaf, 0, 0]
        );
        assert!(super::super::updated_payload(a, &[0x23], 1, false).is_err());
        assert!(super::super::validate_input(&specs[2], "2").is_err());
        assert!(super::super::validate_input(a, "15").is_err());
    }

    #[test]
    fn classic_switch_get_padding_is_not_interpreted_as_function_bits() {
        let specs = schema("ST-9070-L");
        for raw in [
            vec![0x23, 0xaf],
            vec![0x23, 0xaf, 0xff, 0xff],
            vec![0x23, 0xaf, 0x50, 0x7b],
            vec![0x23, 0xaf, 0, 0],
        ] {
            for (spec, value) in specs.iter().zip([2, 3, 10, 15]) {
                validate_reply(spec, &raw).unwrap();
                assert_eq!(spec.decode(&raw).unwrap(), json!(value));
            }
            assert_eq!(
                super::super::updated_payload(&specs[0], &raw, 1, false).unwrap(),
                [0x13, 0xaf, 0, 0]
            );
        }
        assert!(validate_reply(&specs[0], &[0x23]).is_err());
    }

    #[test]
    fn classic_two_button_switch_uses_single_packed_byte() {
        let specs = schema("ST-6770-R");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].query, [0]);
        assert_eq!(specs[0].group, 4);
        assert_eq!((specs[0].set, specs[0].get), (0x10, 0x14));
        assert_eq!(
            super::super::updated_payload(&specs[0], &[0x32], 0, false).unwrap(),
            [0x02]
        );
        let alfine = schema("SW-S705");
        assert_eq!(alfine[0].choices, [2, 3]);
        assert!(super::super::validate_input(&alfine[0], "0").is_err());
        assert!(super::super::validate_input(&alfine[0], "2").is_ok());
    }

    #[test]
    fn derailleur_settings_use_their_own_command_families() {
        let rear = schema("RD-M8050-GS");
        assert_eq!(rear[0].decode(&[0xf4, 0x0c]).unwrap(), json!(-12));
        assert!(rear[0].read_only);
        let speed = rear.iter().find(|s| s.name == "multi-shift-speed").unwrap();
        assert_eq!((speed.group, speed.set, speed.get), (0x0b, 0x50, 0x54));
        assert_eq!(
            super::super::updated_payload(speed, &[3, 0xff, 0xff, 0xff], 4, false).unwrap(),
            [4, 0, 0, 0]
        );
        let front = schema("FD-R9150");
        let interval = front
            .iter()
            .find(|s| s.name == "synchro-shift-interval")
            .unwrap();
        assert_eq!(
            (interval.group, interval.set, interval.get),
            (0x0a, 0x98, 0x9c)
        );
        assert_eq!(interval.template, [0]);
        assert_eq!(
            super::super::updated_payload(interval, &[1, 0xff, 0xff, 0xff], 2, false).unwrap(),
            [2]
        );
        assert!(!schema("FD-6770")
            .iter()
            .any(|s| s.name == "synchro-shift-interval"));
    }

    #[test]
    fn catalog_recognition_does_not_invent_component_settings() {
        for name in [
            "ST-UNKNOWN",
            "SM-PCE1",
            "RD-X8050",
            // Catalogued levers and switches whose layout differs from these schemas.
            "ST-R9250-L",
            "ST-R8150-R",
            "ST-R9270-L",
            "ST-R7170-L",
            "ST-RX825-R",
            "SW-EN600-L",
            "SW-EM800-L",
            "SW-E6010-R",
        ] {
            assert!(schema(name).is_empty(), "{name}");
        }
        for model in crate::unit_models::UNIT_MODELS {
            for spec in schema(model.name) {
                assert!(!spec.oem);
            }
        }
    }

    #[test]
    fn extended_switch_supports_dfly_and_custom_assignments() {
        let specs = schema("ST-R9150-L");
        assert_eq!(specs.len(), 4);
        let a = &specs[0];
        let c = &specs[2];
        assert_eq!(a.choices, vec![0, 1, 2, 3, 10, 11, 12, 13]);
        assert_eq!(c.choices, vec![0, 1, 2, 3, 4, 5, 10, 11, 12, 13, 15]);
        assert!(!c.read_only);

        let original = [0x23, 0x0f, 0, 0];
        assert_eq!(c.decode(&original).unwrap(), json!(0));
        assert_eq!(
            super::super::updated_payload(c, &original, 10, false).unwrap(),
            [0x23, 0xaf, 0, 0]
        );
        assert!(super::super::validate_input(c, "10").is_ok());
        assert!(super::super::validate_input(c, "15").is_ok());
        assert!(super::super::validate_input(c, "6").is_err());
    }

    #[test]
    fn steps_switch_variable_mode_and_button_swap() {
        let e7000 = schema("SW-E7000-L");
        assert_eq!(e7000.len(), 3);
        let mode = &e7000[0];
        let a = &e7000[1];
        let b = &e7000[2];
        assert_eq!(mode.name, "switch-mode");
        assert_eq!(mode.choices, vec![0, 1, 2]);
        assert_eq!(mode.decode(&[0x63, 0, 0, 0]).unwrap(), json!(1));
        assert_eq!(mode.decode(&[0x27, 0, 0, 0]).unwrap(), json!(1));
        assert_eq!(mode.decode(&[0x76, 0, 0, 0]).unwrap(), json!(2));
        assert_eq!(
            super::super::updated_payload(mode, &[0x76, 0, 0, 0], 0, false).unwrap(),
            [0x32, 0, 0, 0]
        );
        assert!(super::super::updated_payload(mode, &[0x63, 0, 0, 0], 1, false).is_err());
        assert!(super::super::validate_new_value(mode, "1.0", false).is_err());

        let assist_payload = [0x67, 0x00, 0x00, 0x00];
        assert_eq!(mode.decode(&assist_payload).unwrap(), json!(2));
        assert_eq!(a.decode(&assist_payload).unwrap(), json!(6));
        assert_eq!(b.decode(&assist_payload).unwrap(), json!(7));

        let shifting_payload =
            super::super::updated_payload(mode, &assist_payload, 0, false).unwrap();
        assert_eq!(shifting_payload, [0x23, 0x00, 0x00, 0x00]);
        assert_eq!(mode.decode(&shifting_payload).unwrap(), json!(0));

        let e6010 = schema("SW-E6010-L");
        assert_eq!(e6010.len(), 4);
        let s = &e6010[3];
        assert_eq!(s.name, "switch-s-function");
        assert_eq!(s.bit_mask, Some(0x0f));
        let third_payload = [0x67, 0x04, 0x00, 0x00];
        assert_eq!(s.decode(&third_payload).unwrap(), json!(4));
        assert_eq!(
            super::super::updated_payload(s, &third_payload, 7, false).unwrap(),
            [0x67, 0x07, 0x00, 0x00]
        );
        assert!(super::super::validate_input(s, "15").is_err());

        // SW-E6000's third button is C, the upper half of byte 1.
        let c = schema("SW-E6000").pop().unwrap();
        assert_eq!(c.name, "switch-c-function");
        assert_eq!(
            super::super::updated_payload(&c, &[0x23, 0x04, 0, 0], 6, false).unwrap(),
            [0x23, 0x64, 0, 0]
        );
    }

    #[test]
    fn steps_switch_choices_follow_each_switch_pattern() {
        let names = |model| {
            schema(model)
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("SW-E8000-L"),
            ["switch-a-function", "switch-b-function"]
        );
        assert_eq!(schema("SW-E8000-L")[0].choices, [6, 7]);
        assert_eq!(
            names("SW-E7000-R"),
            ["switch-a-function", "switch-b-function"]
        );
        assert_eq!(schema("SW-E7000-R")[0].choices, [0, 1, 2, 3]);
        let a = &schema("SW-E7000-L")[1];
        assert_eq!(a.choices, [0, 1, 2, 3, 6, 7]);
        assert!(super::super::validate_input(a, "10").is_err());
        assert!(super::super::validate_input(a, "15").is_err());
    }

    #[test]
    fn twelve_speed_di2_components_have_valid_schemas() {
        let rd = schema("RD-R9250");
        assert_eq!(rd.len(), 3);
        assert!(rd.iter().any(|s| s.name == "multi-shift-speed"));
        assert!(rd.iter().any(|s| s.name == "derailleur-adjustment"));

        let fd = schema("FD-R9250");
        assert_eq!(fd.len(), 3);
        assert!(fd.iter().any(|s| s.name == "synchro-shift-interval"));
    }
}
