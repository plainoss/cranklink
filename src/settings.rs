//! Named component settings with model-specific encodings and validation.
//! All writes preserve shared fields, journal the original before sending, and read back.
use crate::device::UnitCommands;
use crate::storage::OpenOptions;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Write, path::Path};

pub mod batch;
#[cfg(test)]
mod component_tests;
mod components;
mod components_other;

/// Identity selection belongs to the session layer; firmware names alone do not
/// distinguish EP600 from EP801, which share the DUEP600 firmware family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SettingsModel {
    Ep800Crg,
    Ep600,
    Ep801,
    Ep500,
    E5100,
    E8000,
    E7000,
    E6100,
    E6000,
    Component(&'static str),
}
impl SettingsModel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ep800Crg => "DU-EP800/DU-EP800-CRG",
            Self::Ep600 => "DU-EP600/DU-EP600-CRG",
            Self::Ep801 => "DU-EP801/DU-EP801-CRG",
            Self::Ep500 => "DU-EP500",
            Self::E5100 => "DU-E5100",
            Self::E8000 => "DU-E8000/DU-E8080",
            Self::E7000 => "DU-E7000",
            Self::E6100 => "DU-E6100 family",
            Self::E6000 => "DU-E6000 family",
            Self::Component(name) => name,
        }
    }
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Ep800Crg => "ep800-crg",
            Self::Ep600 => "ep600",
            Self::Ep801 => "ep801",
            Self::Ep500 => "ep500",
            Self::E5100 => "e5100",
            Self::E8000 => "e8000",
            Self::E7000 => "e7000",
            Self::E6100 => "e6100",
            Self::E6000 => "e6000",
            Self::Component(name) => name,
        }
    }
    pub(crate) fn series(self) -> u8 {
        match self {
            Self::Ep800Crg => 0x23,
            Self::Ep600 => 0x30,
            Self::Ep801 => 0x35,
            Self::Ep500 => 0x37,
            Self::E5100 => 0x38,
            Self::E8000 => 0x12,
            Self::E7000 => 0x21,
            Self::E6100 => 0x20,
            Self::E6000 => 0x08,
            Self::Component(name) => {
                crate::unit_models::UNIT_MODELS
                    .iter()
                    .find(|entry| entry.name == name)
                    .expect("component settings model belongs to catalog")
                    .series
            }
        }
    }
    fn modern(self) -> bool {
        matches!(self, Self::Ep600 | Self::Ep801 | Self::Ep500)
    }

    /// Resolve offline motor aliases or an exact catalogued component name.
    pub fn from_name(name: &str) -> Result<Self> {
        let model = match name.to_ascii_lowercase().as_str() {
            "ep800-crg" | "ep800" => Self::Ep800Crg,
            "ep600" => Self::Ep600,
            "ep801" => Self::Ep801,
            "ep500" => Self::Ep500,
            "e5100" => Self::E5100,
            "e8000" => Self::E8000,
            "e7000" => Self::E7000,
            "e6100" => Self::E6100,
            "e6000" => Self::E6000,
            _ => {
                let entry = crate::unit_models::UNIT_MODELS.iter()
                    .find(|entry| crate::unit_models::name_matches(name, entry.name))
                    .with_context(|| format!("unknown settings model {name:?}; use a motor family alias or catalogued component name"))?;
                // Drive units require their live product attributes; do not let
                // a catalog name bypass the motor-specific schema selection.
                ensure!(
                    !entry.name.starts_with("DU-"),
                    "use a motor family alias for drive-unit schemas"
                );
                Self::Component(entry.name)
            }
        };
        Ok(model)
    }
}

/// Modern basic-mode settings. Hardware support requires separate validation.
/// The modern profile carries an additional meaningful byte. In particular,
/// byte 5 contains carry-over/cut-off values and must survive sibling edits.
pub fn schema_for_model(model: SettingsModel) -> Vec<SettingSpec> {
    if let SettingsModel::Component(name) = model {
        let mut specs = components::schema(name);
        specs.extend(components_other::schema(name));
        return specs;
    }
    let mut specs = schema();
    if matches!(
        model,
        SettingsModel::E8000 | SettingsModel::E7000 | SettingsModel::E6100 | SettingsModel::E6000
    ) {
        specs.retain(|s| {
            matches!(
                s.name.as_str(),
                "tire-circumference" | "mounting-angle" | "lamp"
            )
        });
        for spec in &mut specs {
            if spec.name == "lamp" {
                spec.max = 1;
                spec.choices = vec![0, 1];
            }
            if spec.name == "mounting-angle" {
                spec.template.truncate(2);
            }
            if model == SettingsModel::E6000 {
                if spec.name == "tire-circumference" {
                    spec.min = 2000;
                    spec.max = 2400;
                }
                if spec.name == "mounting-angle" {
                    spec.max = 45;
                }
            }
        }
        return specs;
    }
    if model == SettingsModel::E5100 {
        specs.retain(|s| {
            matches!(
                s.name.as_str(),
                "lamp"
                    | "lighting-time"
                    | "walk-assist"
                    | "tire-circumference"
                    | "mounting-angle"
                    | "destination"
                    | "speed-limit"
                    | "display-speed-adjustment"
            ) || s.name.starts_with("maintenance-")
        });
        for spec in &mut specs {
            if spec.name == "mounting-angle" {
                spec.template.truncate(2);
            }
        }
        return specs;
    }
    if !model.modern() {
        return specs;
    }
    specs.retain(|s| {
        matches!(
            s.name.as_str(),
            "lamp"
                | "lighting-time"
                | "walk-assist"
                | "tire-circumference"
                | "mounting-angle"
                | "destination"
                | "assist-pattern"
                | "speed-limit"
                | "display-speed-adjustment"
                | "maintenance-alert"
                | "maintenance-distance"
                | "maintenance-year"
                | "maintenance-month"
                | "maintenance-day"
                | "maintenance-date"
        ) || s.name.starts_with("profile")
    });
    for spec in &mut specs {
        if spec.value_type == "string" {
            spec.name_reserved_byte = false;
        }
        if spec.group == 0x17 && spec.set == 0x28 {
            spec.template.truncate(2);
        }
        if spec.group == 0x16 && spec.set == 0xf8 {
            spec.preserve = Some(6);
            if spec.name.contains(".character") {
                (spec.min, spec.max) = match spec.query[1] {
                    1 => (0, 9),
                    2 => (0, 14),
                    _ => (4, 14),
                };
            }
            // Dynamic OEM limits and the model's torque level ranges are checked
            // at SET time, rather than confusing UI level indices with raw Nm.
        }
        if spec.name == "destination" {
            spec.min = 0;
            spec.max = 5;
            spec.choices = vec![0, 1, 2, 3, 5];
        }
        if spec.name == "speed-limit" {
            spec.min = 1400;
            spec.max = 4500;
        }
    }
    specs
}

pub fn capabilities_for_model(model: SettingsModel) -> Value {
    if let SettingsModel::Component(name) = model {
        let specs = schema_for_model(model);
        return json!({"model": name, "ble_hardware_validated": false,
            "usb_baseline_validated": false, "profile_mode": "not applicable",
            "settings_available": !specs.is_empty(),
            "unsupported_features": if specs.is_empty() {
                vec!["named settings are not implemented for this component"]
            } else { vec!["settings outside the model schema"] }});
    }
    json!({"model":model.name(),"ble_hardware_validated":false,"usb_baseline_validated":model == SettingsModel::Ep800Crg,
        "profile_mode":if model.modern(){"basic mode only"}else if model == SettingsModel::Ep800Crg {"legacy profiles"}else{"not implemented"},
        "unsupported_features":if model.modern(){vec!["fine-tune profiles", "free-shift/modern automatic shifting", "OEM light-output and maximum-power tuning", "OEM torque-bound writes"]}else if model == SettingsModel::Ep800Crg {vec![]}else{vec!["assist profiles", "drivetrain and shifting settings", "OEM torque and power tuning"]}})
}

/// One model-specific setting and its parameter layout.
///
/// `query` is the complete GET parameter list, including selectors. `template`
/// is the complete SET parameter list with required reserved/default bytes.
/// `offset` and `width` select the numeric field within reply/SET parameters,
/// after route, group and opcode have been removed; multi-byte values are
/// little-endian. `scale` is raw wire units per displayed input unit.
///
/// Several named settings share one reply. A write copies only its meaningful
/// prefix into the SET template, changes the chosen field, and reconstructs
/// reserved bytes from the template. `preserve` overrides that prefix length
/// for a model; it is not the total reply length, which can include filler.
#[derive(Debug, Clone, Serialize)]
pub struct SettingSpec {
    pub name: String,
    pub unit: String,
    pub value_type: String,
    #[serde(rename = "min_raw")]
    pub min: i64,
    #[serde(rename = "max_raw")]
    pub max: i64,
    #[serde(rename = "step_raw")]
    pub step: i64,
    pub choices: Vec<i64>,
    pub oem: bool,
    pub nullable: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    #[serde(skip)]
    bit_mask: Option<u8>,
    #[serde(skip)]
    group: u8,
    #[serde(skip)]
    get: u8,
    #[serde(skip)]
    set: u8,
    #[serde(skip)]
    query: Vec<u8>,
    #[serde(skip)]
    template: Vec<u8>,
    #[serde(skip)]
    offset: usize,
    #[serde(skip)]
    width: usize,
    #[serde(skip)]
    signed: bool,
    #[serde(rename = "wire_units_per_input")]
    pub scale: f64,
    #[serde(skip)]
    preserve: Option<usize>,
    #[serde(skip)]
    name_reserved_byte: bool,
}
impl SettingSpec {
    // Explicit wire layout columns make the auditable command table readable.
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: &str,
        unit: &str,
        range: (i64, i64, i64),
        group: u8,
        set: u8,
        query: &[u8],
        template: &[u8],
        offset: usize,
        width: usize,
    ) -> Self {
        Self {
            name: name.into(),
            unit: unit.into(),
            value_type: "number".into(),
            min: range.0,
            max: range.1,
            step: range.2,
            choices: vec![],
            oem: false,
            nullable: name.starts_with("maintenance-") && name != "maintenance-alert",
            read_only: false,
            bit_mask: None,
            group,
            get: set + 4,
            set,
            query: query.to_vec(),
            template: template.to_vec(),
            offset,
            width,
            signed: false,
            scale: 1.0,
            preserve: None,
            name_reserved_byte: true,
        }
    }
    fn choices(mut self, v: &[i64]) -> Self {
        self.choices = v.to_vec();
        self
    }
    fn oem(mut self) -> Self {
        self.oem = true;
        self
    }
    fn decode(&self, p: &[u8]) -> Result<Value> {
        if self.value_type == "date" {
            ensure!(p.len() >= 4, "short calendar reply");
            if p[..3].iter().all(|&b| b == 255) {
                return Ok(Value::Null);
            }
            validate_combination(self, p)?;
            return Ok(json!(format!(
                "{:04}-{:02}-{:02}",
                2000 + p[0] as u16,
                p[1],
                p[2]
            )));
        }
        if self.width == 0 {
            return Ok(json!(
                String::from_utf8(p.to_vec())?.trim_end_matches(char::from(0))
            ));
        }
        ensure!(
            p.len() >= self.offset + self.width,
            "short reply for {}: {}",
            self.name,
            hex::encode(p)
        );
        let b = &p[self.offset..self.offset + self.width];
        if self.nullable && b.iter().all(|&x| x == 255) {
            return Ok(Value::Null);
        }
        let mut n = 0i64;
        for (i, x) in b.iter().enumerate() {
            n |= (*x as i64) << (8 * i);
        }
        if let Some(mask) = self.bit_mask {
            n = (n & i64::from(mask)) >> mask.trailing_zeros();
        }
        if self.signed && b[self.width - 1] & 128 != 0 {
            n -= 1i64 << (8 * self.width);
        }
        if self.scale == 1.0 {
            Ok(json!(n))
        } else {
            Ok(json!(n as f64 / self.scale))
        }
    }
    fn raw_value(&self, value: &str) -> Result<i64> {
        let f: f64 = value.parse().context("setting value must be numeric")?;
        ensure!(f.is_finite(), "value must be finite");
        let raw = f * self.scale;
        ensure!(
            (raw - raw.round()).abs() < 0.00001,
            "value has too many decimal places"
        );
        let n = raw.round() as i64;
        ensure!(
            (self.min..=self.max).contains(&n),
            "{} must be {}..={} {}",
            self.name,
            self.min as f64 / self.scale,
            self.max as f64 / self.scale,
            self.unit
        );
        ensure!(
            (n - self.min) % self.step == 0,
            "{} requires increments of {} {}",
            self.name,
            self.step as f64 / self.scale,
            self.unit
        );
        ensure!(
            self.choices.is_empty() || self.choices.contains(&n),
            "{} must be one of {:?}",
            self.name,
            self.choices
        );
        Ok(n)
    }
}
/// Base EP800 layouts; use `schema_for_model` for another supported drive unit.
/// In assist tuples, profiles 0/1 are OEM defaults and 2/3 are user profiles;
/// modes 1/2/3 mean ECO / TRAIL-or-NORMAL / BOOST-or-HIGH. Torque is raw Nm,
/// not a five-Nm index (for example, 27 Nm is representable). OEM torque-bound
/// controls have their own step constraints, separate from per-mode torque.
pub fn schema() -> Vec<SettingSpec> {
    let mut v = vec![
        SettingSpec::new("lamp", "enum", (0, 2, 1), 0x10, 0x10, &[0], &[0], 0, 1),
        SettingSpec::new(
            "light-output",
            "0.01 W",
            (135, 2700, 1),
            0x10,
            0x18,
            &[0],
            &[0, 0, 255, 255],
            0,
            2,
        )
        .oem(),
        SettingSpec::new(
            "lighting-time",
            "minutes",
            (0, 180, 1),
            0x16,
            0xa0,
            &[0],
            &[0, 0, 255, 255],
            0,
            2,
        ),
        SettingSpec::new(
            "walk-assist",
            "enum",
            (0, 2, 1),
            0x16,
            0x68,
            &[0],
            &[0],
            0,
            1,
        )
        .choices(&[0, 2])
        .oem(),
        SettingSpec::new(
            "tire-circumference",
            "mm",
            (1300, 3000, 1),
            0x35,
            0,
            &[0],
            &[0, 0, 0, 0],
            0,
            2,
        )
        .oem(),
        SettingSpec::new(
            "mounting-angle",
            "degrees",
            (0, 60, 1),
            0x16,
            0x58,
            &[0],
            &[0, 0, 0, 0],
            0,
            2,
        )
        .oem(),
        SettingSpec::new(
            "destination",
            "enum",
            (0, 4, 1),
            0x16,
            0xa8,
            &[1],
            &[1, 0, 255, 255],
            1,
            1,
        )
        .choices(&[0, 1, 3, 4])
        .oem(),
        SettingSpec::new(
            "assist-pattern",
            "profile",
            (5, 6, 1),
            0x16,
            0x80,
            &[0],
            &[0, 0, 0, 0],
            0,
            1,
        ),
        SettingSpec::new(
            "speed-limit",
            "km/h",
            (1500, 3200, 100),
            0x16,
            0xb0,
            &[1],
            &[0, 0, 1, 255],
            0,
            2,
        ),
        SettingSpec::new(
            "display-speed-adjustment",
            "0.1 percent",
            (-50, 50, 5),
            0x16,
            0xd0,
            &[0],
            &[0],
            0,
            1,
        ),
        SettingSpec::new(
            "shifting-advice",
            "bool",
            (0, 1, 1),
            0x0b,
            0xd8,
            &[0],
            &[0],
            0,
            1,
        ),
        SettingSpec::new(
            "auto-gear-mode",
            "enum",
            (0, 1, 1),
            0x0b,
            0,
            &[0],
            &[0],
            0,
            1,
        ),
        SettingSpec::new(
            "maintenance-alert",
            "bool",
            (0, 1, 1),
            0x48,
            0x40,
            &[0],
            &[0],
            0,
            1,
        ),
        SettingSpec::new(
            "maintenance-distance",
            "km",
            (0, 99999, 1),
            0x48,
            0x48,
            &[0],
            &[0, 0, 0, 0],
            0,
            4,
        ),
        SettingSpec::new(
            "maintenance-year",
            "year minus 2000",
            (0, 99, 1),
            0x48,
            0x50,
            &[0],
            &[0, 1, 1, 0],
            0,
            1,
        ),
        SettingSpec::new(
            "maintenance-month",
            "month",
            (1, 12, 1),
            0x48,
            0x50,
            &[0],
            &[0, 1, 1, 0],
            1,
            1,
        ),
        SettingSpec::new(
            "maintenance-day",
            "day",
            (1, 31, 1),
            0x48,
            0x50,
            &[0],
            &[0, 1, 1, 0],
            2,
            1,
        ),
        SettingSpec::new(
            "torque-minimum",
            "Nm",
            (20, 40, 5),
            0x17,
            0x18,
            &[0],
            &[0, 0, 0, 0],
            0,
            1,
        )
        .oem(),
        SettingSpec::new(
            "torque-maximum",
            "Nm",
            (30, 85, 5),
            0x17,
            0x18,
            &[0],
            &[0, 0, 0, 0],
            1,
            1,
        )
        .oem(),
    ];
    for s in &mut v {
        if s.name.starts_with("speed-limit") {
            s.scale = 100.0;
        }
        if s.name == "display-speed-adjustment" {
            s.signed = true;
        }
    }
    for (profile, p) in [(1, 2u8), (2, 3u8), (1, 0u8), (2, 1u8)] {
        let suffix = if p < 2 { "-oem" } else { "" };
        for (mode, m) in [("eco", 1u8), ("trail", 2), ("boost", 3)] {
            for (field, offset, range) in [
                ("torque", 2, (20, 85, 1)),
                ("character", 3, (0, 9, 1)),
                ("start", 4, (0, 4, 1)),
            ] {
                let mut s = SettingSpec::new(
                    &format!("profile{profile}.{mode}.{field}{suffix}"),
                    if field == "torque" { "Nm" } else { "raw index" },
                    range,
                    0x16,
                    0xf8,
                    &[p, m],
                    &[p, m, 0, 0, 0, 0, 0],
                    offset,
                    1,
                );
                s.oem = p < 2;
                v.push(s);
            }
        }
        // Display mode: 0=ECO/NORMAL/HIGH, 1=ECO/TRAIL/BOOST.
        let mut display = SettingSpec::new(
            &format!("profile{profile}.display{suffix}"),
            "enum",
            (0, 1, 1),
            0x17,
            0x28,
            &[p],
            &[p, 0, 0, 0],
            1,
            1,
        );
        display.oem = p < 2;
        v.push(display);
    }
    v.push(
        SettingSpec::new(
            "rear-teeth-pattern",
            "enum",
            (0, 11, 1),
            0x0b,
            0xb0,
            &[0],
            &[0, 0, 255, 255],
            0,
            2,
        )
        .choices(&[0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11])
        .oem(),
    );
    let mut cadence = SettingSpec::new(
        "auto-gear-cadence",
        "rpm",
        (500, 760, 10),
        0x0b,
        0x90,
        &[0],
        &[0, 0, 0, 0, 0, 0, 0],
        0,
        2,
    );
    cadence.scale = 10.0;
    v.push(cadence);
    let mut date = SettingSpec::new(
        "maintenance-date",
        "ISO YYYY-MM-DD or unset",
        (0, 0, 1),
        0x48,
        0x50,
        &[0],
        &[0, 0, 0, 0],
        0,
        0,
    );
    date.value_type = "date".into();
    v.push(date);
    for profile in 1..=2 {
        let mut n = SettingSpec::new(
            &format!("profile{profile}.name"),
            "ASCII, up to 10 characters",
            (0, 0, 1),
            0x17,
            0x20,
            &[if profile == 1 { 0x50 } else { 0x60 }],
            &[],
            0,
            0,
        );
        n.value_type = "string".into();
        v.push(n);
    }
    v.extend([
        SettingSpec::new(
            "shifting-system",
            "enum",
            (0, 5, 1),
            0x0b,
            0x80,
            &[0],
            &[0, 0, 0, 0],
            0,
            1,
        )
        .oem(),
        SettingSpec::new(
            "rear-max-gear",
            "gears",
            (5, 12, 1),
            0x0b,
            0x80,
            &[0],
            &[0, 0, 0, 0],
            1,
            1,
        )
        .choices(&[5, 7, 8, 9, 10, 11, 12])
        .oem(),
        SettingSpec::new(
            "front-teeth",
            "teeth",
            (30, 60, 1),
            0x0a,
            0x48,
            &[0],
            &[1, 0, 0, 0],
            1,
            1,
        )
        .oem(),
        SettingSpec::new(
            "rear-teeth",
            "teeth",
            (10, 14, 1),
            0x0b,
            0x48,
            &[0],
            &[0, 0, 0],
            1,
            1,
        )
        .oem(),
        SettingSpec::new(
            "start-mode",
            "gear, 0=off",
            (0, 11, 1),
            0x0b,
            0x88,
            &[0],
            &[0, 0, 0, 0],
            0,
            1,
        ),
        SettingSpec::new(
            "shift-limit-up",
            "gears, 255=unlimited",
            (2, 255, 1),
            0x0b,
            0x60,
            &[0],
            &[0, 0, 0, 0],
            0,
            1,
        )
        .choices(&[2, 3, 255]),
        SettingSpec::new(
            "shift-limit-down",
            "gears, 255=unlimited",
            (2, 255, 1),
            0x0b,
            0x60,
            &[0],
            &[0, 0, 0, 0],
            1,
            1,
        )
        .choices(&[2, 3, 255]),
    ]);
    for (mode, m) in [("eco", 1u8), ("trail", 2), ("boost", 3)] {
        v.push(
            SettingSpec::new(
                &format!("max-power.{mode}"),
                "W",
                (100, 500, 50),
                0x17,
                0x10,
                &[m],
                &[m, 0, 0, 0],
                1,
                2,
            )
            .oem(),
        );
    }
    v
}
fn spec(name: &str) -> Result<SettingSpec> {
    spec_for_model(name, SettingsModel::Ep800Crg)
}
fn spec_for_model(name: &str, model: SettingsModel) -> Result<SettingSpec> {
    schema_for_model(model)
        .into_iter()
        .find(|s| s.name == name)
        .with_context(|| {
            format!(
                "setting {name} is unsupported for {}; use model settings schema",
                model.name()
            )
        })
}
#[derive(Debug, Serialize)]
pub struct SettingValue {
    pub value: Value,
    pub raw: String,
}
#[derive(Debug, Serialize)]
pub struct SettingsSnapshot {
    pub model: String,
    pub slot: u8,
    pub values: BTreeMap<String, SettingValue>,
    pub unavailable: BTreeMap<String, String>,
}
/// Compare complete decoded settings, ignoring unused bytes in bus replies.
/// Stock replies retain unrelated trailing bytes across firmware transitions.
pub fn decoded_snapshots_equal(before: &Value, after: &Value) -> bool {
    fn decoded(snapshot: &Value) -> Option<BTreeMap<&str, &Value>> {
        if !snapshot.get("unavailable")?.as_object()?.is_empty() {
            return None;
        }
        let values = snapshot.get("values")?.as_object()?;
        if values.is_empty() {
            return None;
        }
        values
            .iter()
            .map(|(name, value)| Some((name.as_str(), value.get("value")?)))
            .collect()
    }
    match (decoded(before), decoded(after)) {
        (Some(before), Some(after)) => before == after,
        _ => false,
    }
}
pub fn read(session: &mut dyn UnitCommands, slot: u8, name: &str) -> Result<SettingValue> {
    read_for_model(session, slot, name, SettingsModel::Ep800Crg)
}
pub fn read_for_model(
    session: &mut dyn UnitCommands,
    slot: u8,
    name: &str,
    model: SettingsModel,
) -> Result<SettingValue> {
    let s = spec_for_model(name, model)?;
    let raw = read_raw(session, slot, &s)?;
    validate_model_reply(&s, &raw)?;
    Ok(SettingValue {
        value: s.decode(&raw)?,
        raw: hex::encode(raw),
    })
}
pub(crate) fn supports_command(group: u8, command: u8) -> bool {
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
    ]
    .into_iter()
    .chain(
        crate::unit_models::UNIT_MODELS
            .iter()
            .filter(|entry| !entry.name.starts_with("DU-"))
            .map(|entry| SettingsModel::Component(entry.name)),
    )
    .any(|model| {
        schema_for_model(model)
            .iter()
            .any(|s| s.group == group && (s.get == command || (!s.read_only && s.set == command)))
    })
}
/// Profile names use two five-byte text chunks, selected by hex `50/51` or
/// `60/61`. GET `17/24 [selector]` returns `[selector, ASCII5, ...]`; only the
/// five text bytes belong to the name. SET `17/20` uses `[selector, ASCII5, 00]`
/// in the legacy schema and `[selector, ASCII5]` in the modern schema. Short
/// names are zero-filled to ten bytes before the two chunks are written.
fn read_raw(session: &mut dyn UnitCommands, slot: u8, s: &SettingSpec) -> Result<Vec<u8>> {
    if s.width != 0 || s.value_type == "date" {
        return session.command(slot, s.group, s.get, &s.query, s.get + 2);
    }
    let mut name = Vec::new();
    for chunk in 0..2 {
        let selector = s.query[0] | chunk;
        let p = session.command(slot, 0x17, 0x24, &[selector], 0x26)?;
        ensure!(
            p.len() >= 6 && p[0] == selector,
            "invalid profile-name chunk"
        );
        name.extend_from_slice(&p[1..6]);
    }
    Ok(name)
}
pub fn read_all(session: &mut dyn UnitCommands, slot: u8) -> SettingsSnapshot {
    read_all_for_model(session, slot, SettingsModel::Ep800Crg)
}
pub fn read_all_for_model(
    session: &mut dyn UnitCommands,
    slot: u8,
    model: SettingsModel,
) -> SettingsSnapshot {
    let mut values = BTreeMap::new();
    let mut unavailable = BTreeMap::new();
    // Cache by command/query so shared profile fields do not trigger duplicate requests.
    type ReadCache = BTreeMap<(u8, u8, Vec<u8>), std::result::Result<Vec<u8>, String>>;
    let mut cache: ReadCache = BTreeMap::new();
    for s in schema_for_model(model) {
        if s.width == 0 {
            match read_for_model(session, slot, &s.name, model) {
                Ok(value) => {
                    values.insert(s.name, value);
                }
                Err(e) => {
                    unavailable.insert(s.name, e.to_string());
                }
            }
            continue;
        }
        let r = cache
            .entry((s.group, s.get, s.query.clone()))
            .or_insert_with(|| {
                session
                    .command(slot, s.group, s.get, &s.query, s.get + 2)
                    .map_err(|e| e.to_string())
            });
        match r {
            Ok(raw) => match validate_model_reply(&s, raw).and_then(|_| s.decode(raw)) {
                Ok(value) => {
                    values.insert(
                        s.name,
                        SettingValue {
                            value,
                            raw: hex::encode(raw),
                        },
                    );
                }
                Err(e) => {
                    unavailable.insert(s.name, e.to_string());
                }
            },
            Err(e) => {
                unavailable.insert(s.name, e.clone());
            }
        }
    }
    SettingsSnapshot {
        model: model.name().into(),
        slot,
        values,
        unavailable,
    }
}
#[derive(Debug, Serialize)]
pub struct ChangeReceipt {
    pub setting: String,
    pub before: Value,
    pub after: Value,
    pub backup: String,
    pub changed: bool,
}
pub fn set(
    session: &mut dyn UnitCommands,
    slot: u8,
    name: &str,
    value: &str,
    backup_path: &Path,
) -> Result<ChangeReceipt> {
    set_for_model(
        session,
        slot,
        name,
        value,
        backup_path,
        SettingsModel::Ep800Crg,
    )
}

/// Readbacks after a hub adjustment SET while its actuator moves.
const HUB_COMPLETION_POLLS: usize = 30;

/// Back up, issue the model-specific SET, and verify the current value by GET.
/// The implemented setting path sends no additional generic EEPROM commit.
/// A successful readback confirms the live value; persistence across power
/// cycles needs a separate GET after the component restarts.
pub fn set_for_model(
    session: &mut dyn UnitCommands,
    slot: u8,
    name: &str,
    value: &str,
    backup_path: &Path,
    model: SettingsModel,
) -> Result<ChangeReceipt> {
    let s = spec_for_model(name, model)?;
    validate_input(&s, value)?;
    if s.value_type == "date" {
        return set_date(session, slot, &s, value, backup_path);
    }
    if s.width == 0 {
        return set_name(session, slot, &s, value, backup_path);
    }
    let unset = value == "unset";
    ensure!(!unset || s.nullable, "this setting does not support unset");
    let n = if unset {
        (1i64 << (8 * s.width)) - 1
    } else {
        s.raw_value(value)?
    };
    let original = session.command(slot, s.group, s.get, &s.query, s.get + 2)?;
    validate_model_reply(&s, &original)?;
    let before = s.decode(&original)?;
    validate_current_value(model, &s, &before)?;
    let payload = updated_payload(&s, &original, n, unset)?;
    validate_combination(&s, &payload).context("invalid setting combination; to initialize an unset calendar use maintenance-date YYYY-MM-DD")?;
    if s.name.starts_with("max-power.") {
        let destination = effective_destination(session, slot, model)?;
        ensure!(
            destination != 3 || n <= 400,
            "destination 3 limits maximum power to 400 W"
        );
    }
    if s.name.contains(".torque") || s.name == "torque-maximum" {
        let drivetrain = session.command(slot, 0x0b, 0x84, &[0], 0x86)?;
        ensure!(drivetrain.len() >= 2, "short drivetrain reply");
        if matches!(drivetrain[0], 1 | 3 | 4 | 5) && drivetrain[1] != 5 {
            ensure!(
                n <= 50,
                "internal drivetrain with other than 5 gears limits torque to 50 Nm"
            );
        }
    }
    if s.name.contains(".torque") && (!s.oem || model.modern()) {
        let limits = session.command(slot, 0x17, 0x1c, &[0], 0x1e)?;
        ensure!(limits.len() >= 2, "short torque bounds reply");
        ensure!(
            n >= limits[0] as i64 && n <= limits[1] as i64,
            "requested torque is outside OEM bounds {}..{} Nm",
            limits[0],
            limits[1]
        );
        if model.modern() {
            validate_modern_torque(n, s.query[1], limits[0], limits[1])?;
        }
    }
    if model.modern() && s.group == 0x16 && s.set == 0xf8 && s.name.contains(".start") && !s.oem {
        let factory = session.command(slot, 0x16, 0xfc, &[s.query[0] - 2, s.query[1]], 0xfe)?;
        ensure!(
            factory.len() >= 7 && factory[0] == s.query[0] - 2 && factory[1] == s.query[1],
            "invalid OEM basic profile reply"
        );
        ensure!(
            n <= factory[4] as i64,
            "assist start exceeds OEM default index {}",
            factory[4]
        );
    }
    if s.name == "start-mode" {
        let drivetrain = session.command(slot, 0x0b, 0x84, &[0], 0x86)?;
        ensure!(drivetrain.len() >= 2, "short drivetrain reply");
        ensure!(
            matches!(drivetrain[0], 3 | 5),
            "start mode requires electronic internal gearing"
        );
        ensure!(
            n <= drivetrain[1] as i64,
            "start gear exceeds configured number of gears"
        );
    }

    if s.name == "speed-limit" {
        let destination = effective_destination(session, slot, model)?;
        if model.modern() {
            validate_modern_speed_destination(n, destination)?;
        } else {
            validate_speed_destination(n, destination)?;
        }
    }
    let after = s.decode(&payload)?;
    // create_new prevents accidentally overwriting the only known original configuration.
    let mut backup = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup_path)
        .context("create settings backup (choose a new filename)")?;
    serde_json::to_writer_pretty(
        &mut backup,
        &json!({"format":"cranklink-setting-backup-v1","serial":hex::encode(crate::device::serial_bytes(&session.command(slot,1,0x3c,&[0],0x3e)?)?),"slot":slot,"settings_model":model.slug(),"setting":name,"before":before,"original_parameters":hex::encode(&original),"planned_parameters":hex::encode(&payload),"group":s.group,"command":s.set,"query":s.query}),
    )?;
    backup.write_all(b"\n")?;
    backup.sync_all()?;
    let changed = before != after;
    // A previous adjustment may still be moving toward another target even
    // when GET reports the requested current value. Always apply this target
    // once, so restoring an equal current value also cancels pending movement.
    if changed || s.name == "hub-adjustment" {
        session
            .command(slot, s.group, s.set, &payload, s.set + 2)
            .context(
                "setting write failed; outcome may be uncertain, inspect backup and read device",
            )?;
        let started = crate::platform::Instant::now();
        let mut polls = 0;
        let got = loop {
            let got = session
                .command(slot, s.group, s.get, &s.query, s.get + 2)
                .context("write acknowledged but readback failed; inspect backup")?;
            validate_model_reply(&s, &got).context("invalid setting readback; inspect backup")?;
            // Hub adjustment acknowledges acceptance before its actuator has
            // reached the target. Read until completion; never resend the SET.
            if s.name != "hub-adjustment" || s.decode(&got)? == after {
                break got;
            }
            // About three seconds of 100 ms polls; the count also bounds the
            // loop when the transport answers instantly.
            polls += 1;
            ensure!(polls < HUB_COMPLETION_POLLS && started.elapsed() < std::time::Duration::from_secs(3),
                "hub adjustment readback mismatch: actuator completion unconfirmed; outcome may be uncertain; original preserved in {}", backup_path.display());
            crate::platform::sleep(std::time::Duration::from_millis(100));
        };
        ensure!(
            s.decode(&got)? == after,
            "setting readback mismatch; original preserved in {}",
            backup_path.display()
        );
        if let Some(mask) = s.bit_mask {
            ensure!(
                got[s.offset] & !mask == original[s.offset] & !mask,
                "shared setting bits changed unexpectedly"
            );
        }
        // Unsetting any calendar component clears the whole date; like decode()
        // and restore_date, only year, month and day (bytes 0..3) are meaningful.
        let calendar_unset = unset && s.group == 0x48 && s.set == 0x50;
        if calendar_unset {
            ensure!(
                got.get(..3)
                    .is_some_and(|date| date.iter().all(|&b| b == 0xff)),
                "maintenance calendar did not clear; original preserved in {}",
                backup_path.display()
            );
        }
        for i in 0..s.offset {
            ensure!(
                calendar_unset || got.get(i) == original.get(i),
                "shared setting prefix changed unexpectedly"
            );
        }
        for i in s.offset + s.width..meaningful_len(&s).min(original.len()) {
            if (s.name.starts_with("speed-limit") && i >= 2) || calendar_unset {
                continue;
            }
            ensure!(
                got.get(i) == original.get(i),
                "shared setting sibling changed unexpectedly"
            );
        }
    }
    Ok(ChangeReceipt {
        setting: name.into(),
        before,
        after,
        backup: backup_path.display().to_string(),
        changed,
    })
}

// Read OEM destination first. FF means unset, not EU. Modern models fall back
// to the factory selector when unset; legacy EP800 rejects that selector.
// Never infer region from speed.
fn effective_destination(
    session: &mut dyn UnitCommands,
    slot: u8,
    model: SettingsModel,
) -> Result<u8> {
    let selectors: &[u8] = if model.modern() { &[1, 0] } else { &[1] };
    for &selector in selectors {
        let reply = session.command(slot, 0x16, 0xac, &[selector], 0xae)?;
        ensure!(
            reply.len() >= 2 && reply[0] == selector,
            "invalid destination reply for selector {selector}"
        );
        if reply[1] != 0xff {
            return Ok(reply[1]);
        }
    }
    anyhow::bail!(if model.modern() {
        "OEM and factory destinations are unset; cannot validate setting range"
    } else {
        "EP800 OEM destination is unset (255); factory destination GET selector 0 is unsupported by the verified legacy firmware. Set an explicit destination using drive-unit authentication before changing speed or maximum power"
    })
}

fn validate_speed_destination(n: i64, destination: u8) -> Result<()> {
    // On tested EP800 firmware, speed GET 16/B4 selector 00 aliases effective
    // speed rather than returning an independent OEM ceiling. Validate permitted
    // maxima against the model and destination table, not that readback.
    let (minimum, maximum) = match destination {
        1 => (1900, 3200),
        0 | 3 | 4 => (1500, 2500),
        _ => anyhow::bail!("unsupported destination for speed settings: {destination}"),
    };
    ensure!(
        (minimum..=maximum).contains(&n),
        "destination {destination} permits {}..{} km/h",
        minimum / 100,
        maximum / 100
    );
    Ok(())
}

fn validate_modern_speed_destination(n: i64, destination: u8) -> Result<()> {
    // JP=2 and US_CLASS3=5 use different ranges from the EP800 model table.
    let (minimum, maximum) = match destination {
        0 | 3 => (1500, 2500),
        1 => (1900, 3200),
        2 => (1400, 2400),
        5 => (1900, 4500),
        _ => anyhow::bail!("unsupported modern destination {destination}"),
    };
    ensure!(
        (minimum..=maximum).contains(&n),
        "destination {destination} permits {}..{} km/h",
        minimum / 100,
        maximum / 100
    );
    Ok(())
}

fn validate_model_reply(spec: &SettingSpec, raw: &[u8]) -> Result<()> {
    components::validate_reply(spec, raw)?;
    components_other::validate_reply(spec, raw)?;
    if spec.preserve == Some(6) {
        ensure!(
            raw.len() >= 7 && raw[..2] == spec.query[..2],
            "invalid modern basic-mode profile selector/length; fine-tune profiles are unsupported"
        );
    }
    Ok(())
}

fn modern_torque_levels(minimum: u8, maximum: u8) -> Result<[i64; 10]> {
    ensure!(
        minimum >= 20 && minimum <= maximum && maximum <= 85,
        "unsupported modern OEM torque bounds {minimum}..{maximum}"
    );
    Ok(match (minimum, maximum) {
        (20, 85) => [20, 27, 34, 42, 49, 56, 63, 70, 78, 85],
        (20, 50) => [20, 24, 27, 30, 33, 36, 40, 43, 46, 50],
        _ => std::array::from_fn(|i| {
            ((i as f64 * maximum as f64 + (9 - i) as f64 * minimum as f64) / 9.0).round() as i64
        }),
    })
}
fn validate_modern_torque(n: i64, mode: u8, minimum: u8, maximum: u8) -> Result<()> {
    // EP600/EP801 torque levels: ECO 1..8, TRAIL 1..10, BOOST 3..10.
    let levels = modern_torque_levels(minimum, maximum)?;
    let permitted = match mode {
        1 => &levels[..8],
        2 => &levels[..],
        3 => &levels[2..],
        _ => anyhow::bail!("unsupported basic assist mode"),
    };
    ensure!(
        permitted.contains(&n),
        "torque must match this model's basic-mode levels {:?} Nm",
        permitted
    );
    Ok(())
}

// GET often fills reserved bytes with FF while SET requires model-specific 00/FF defaults.
// These lengths preserve sibling fields, not arbitrary trailing reply padding.
fn meaningful_len(s: &SettingSpec) -> usize {
    if let Some(length) = s.preserve {
        return length;
    }
    match (s.group, s.set) {
        (0x10, 0x10)
        | (0x16, 0x68)
        | (0x16, 0x80)
        | (0x16, 0xd0)
        | (0x0b, 0)
        | (0x0b, 0xd8)
        | (0x48, 0x40) => 1,
        (0x16, 0xf8) => 5,
        (0x17, 0x10) => 3,
        (0x0b, 0x88) => 2,
        (0x0b, 0x90) => 6,
        (0x0b, 0x60) | (0x48, 0x48) => 4,
        (0x48, 0x50) => 3,
        _ => 2,
    }
}
fn updated_payload(s: &SettingSpec, original: &[u8], n: i64, unset: bool) -> Result<Vec<u8>> {
    let meaningful = meaningful_len(s);
    ensure!(original.len() >= meaningful, "short shared settings reply");
    ensure!(
        s.template.len() >= meaningful && s.offset + s.width <= s.template.len(),
        "invalid settings wire layout for {}",
        s.name
    );
    let mut payload = s.template.clone();
    payload[..meaningful].copy_from_slice(&original[..meaningful]);
    if s.name.starts_with("speed-limit") {
        payload[2] = s.query[0];
        payload[3] = 255;
    }
    if let Some(mask) = s.bit_mask {
        payload[s.offset] =
            (payload[s.offset] & !mask) | (((n as u8) << mask.trailing_zeros()) & mask);
    } else {
        for i in 0..s.width {
            payload[s.offset + i] = ((n >> (8 * i)) & 255) as u8;
        }
    }
    if unset && s.group == 0x48 && s.set == 0x50 {
        payload.fill(255);
    }
    Ok(payload)
}

fn validate_combination(s: &SettingSpec, p: &[u8]) -> Result<()> {
    if s.group == 0x17 && s.set == 0x18 {
        ensure!(p[0] <= p[1], "minimum torque exceeds maximum");
    }
    if s.group == 0x48 && s.set == 0x50 {
        if p.iter().all(|&b| b == 255) {
            return Ok(());
        }
        let y = 2000 + p[0] as u32;
        let m = p[1];
        let d = p[2];
        let days = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if y.is_multiple_of(4) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        };
        ensure!(d >= 1 && d <= days, "invalid maintenance date");
    }
    Ok(())
}
/// Whether a setting requires BLE master routing (PC owner distinct from the cycle computer).
/// This is distinct from `oem` regulation authentication: `destination` and `tire-circumference`
/// require both drive-unit authentication and master routing, `lamp` and `lighting-time` require
/// master routing without drive-unit authentication, and other `oem` settings require drive-unit
/// authentication without master routing.
pub fn requires_ble_master_routing(setting_name: &str) -> bool {
    matches!(
        setting_name.to_ascii_lowercase().as_str(),
        "destination" | "tire-circumference" | "lamp" | "lighting-time"
    )
}

/// Validated restore input parsed before opening a session so routing/authentication
/// planning and execution use the exact same snapshot without re-reading disk.
#[derive(Debug, Clone)]
pub struct ParsedRestore {
    pub serial: String,
    pub setting: String,
    pub before: Value,
    pub original_parameters: Vec<u8>,
    pub raw_backup: Value,
}

impl ParsedRestore {
    pub fn planned_value(&self) -> String {
        match &self.before {
            Value::Null => "unset".into(),
            Value::String(s) => s.clone(),
            value => value.to_string(),
        }
    }

    pub fn validate_for_model(
        &self,
        model: SettingsModel,
        expected_serial: &str,
    ) -> Result<SettingSpec> {
        ensure!(
            self.serial.eq_ignore_ascii_case(expected_serial),
            "backup belongs to another motor or has no serial identity"
        );
        let spec = spec_for_model(&self.setting, model)?;
        if matches!(model, SettingsModel::Component(_)) {
            ensure!(
                self.raw_backup["settings_model"].as_str() == Some(model.slug()),
                "backup belongs to another component model or lacks its model identity"
            );
        }
        validate_model_reply(&spec, &self.original_parameters)?;
        ensure!(
            spec.decode(&self.original_parameters)? == self.before,
            "backup value does not match captured original parameters"
        );
        if !(spec.group == 0x48 && spec.set == 0x50) {
            validate_input(&spec, &self.planned_value())?;
        }
        Ok(spec)
    }
}

pub fn parse_restore_file(backup_path: &Path) -> Result<ParsedRestore> {
    let backup: Value =
        serde_json::from_slice(&crate::storage::read_bounded(backup_path, 64 * 1024)?)?;
    ensure!(
        backup["format"] == "cranklink-setting-backup-v1"
            || backup["format"] == "esteps-setting-backup-v1",
        "unsupported backup format"
    );
    let serial = backup["serial"]
        .as_str()
        .and_then(|s| hex::decode(s).ok())
        .and_then(|bytes| crate::device::serial_bytes(&bytes).ok())
        .map(hex::encode)
        .context("backup belongs to another motor or has no serial identity")?;
    let setting = backup["setting"]
        .as_str()
        .context("backup missing setting")?
        .to_string();
    let original_parameters = hex::decode(
        backup["original_parameters"]
            .as_str()
            .context("backup lacks original parameters")?,
    )?;
    let before = backup["before"].clone();
    Ok(ParsedRestore {
        serial,
        setting,
        before,
        original_parameters,
        raw_backup: backup,
    })
}

/// Restore only the named field, retaining any independent settings changed since the backup.
pub fn restore(
    session: &mut dyn UnitCommands,
    slot: u8,
    backup_path: &Path,
    new_backup_path: &Path,
) -> Result<ChangeReceipt> {
    restore_for_model(
        session,
        slot,
        backup_path,
        new_backup_path,
        SettingsModel::Ep800Crg,
    )
}
pub fn restore_for_model(
    session: &mut dyn UnitCommands,
    slot: u8,
    backup_path: &Path,
    new_backup_path: &Path,
    model: SettingsModel,
) -> Result<ChangeReceipt> {
    let parsed = parse_restore_file(backup_path)?;
    restore_parsed_for_model(session, slot, &parsed, new_backup_path, model)
}
pub fn restore_parsed_for_model(
    session: &mut dyn UnitCommands,
    slot: u8,
    parsed: &ParsedRestore,
    new_backup_path: &Path,
    model: SettingsModel,
) -> Result<ChangeReceipt> {
    let serial = hex::encode(crate::device::serial_bytes(&session.command(
        slot,
        1,
        0x3c,
        &[0],
        0x3e,
    )?)?);
    let setting = parsed.validate_for_model(model, &serial)?;
    if setting.group == 0x48 && setting.set == 0x50 {
        return restore_date(session, slot, &setting, &parsed.raw_backup, new_backup_path);
    }
    let value = parsed.planned_value();
    set_for_model(
        session,
        slot,
        &parsed.setting,
        &value,
        new_backup_path,
        model,
    )
}

fn parse_date(value: &str) -> Result<[u8; 4]> {
    if value == "unset" {
        return Ok([255; 4]);
    }
    ensure!(
        value.is_ascii()
            && value.len() == 10
            && value.as_bytes()[4] == b'-'
            && value.as_bytes()[7] == b'-',
        "maintenance-date expects YYYY-MM-DD or unset"
    );
    let year: u16 = value[..4].parse()?;
    let month: u8 = value[5..7].parse()?;
    let day: u8 = value[8..].parse()?;
    ensure!(
        (2000..=2099).contains(&year),
        "maintenance year must be 2000..2099"
    );
    let p = [(year - 2000) as u8, month, day, 0];
    validate_combination(&spec("maintenance-date")?, &p)?;
    Ok(p)
}
fn set_date(
    session: &mut dyn UnitCommands,
    slot: u8,
    s: &SettingSpec,
    value: &str,
    path: &Path,
) -> Result<ChangeReceipt> {
    let p = parse_date(value)?;
    // This private path only accepts a validated calendar or the vendor's all-FF unset tuple.
    restore_date(
        session,
        slot,
        s,
        &json!({"original_parameters":hex::encode(p)}),
        path,
    )
}

// Calendar components share one validity constraint: restore the entire captured date.
fn restore_date(
    session: &mut dyn UnitCommands,
    slot: u8,
    s: &SettingSpec,
    backup: &Value,
    path: &Path,
) -> Result<ChangeReceipt> {
    let original = hex::decode(
        backup["original_parameters"]
            .as_str()
            .context("date backup lacks original parameters")?,
    )?;
    ensure!(original.len() >= 4, "short original date");
    let mut original = original[..4].to_vec();
    if !original.iter().all(|&b| b == 255) {
        original[3] = 0;
    }
    let original = original.as_slice();
    validate_combination(s, original)?;
    let current = session.command(slot, 0x48, 0x54, &[0], 0x56)?;
    ensure!(current.len() >= 4, "short current date");
    let before = s.decode(&current)?;
    let after = s.decode(original)?;
    let serial = hex::encode(crate::device::serial_bytes(&session.command(
        slot,
        1,
        0x3c,
        &[0],
        0x3e,
    )?)?);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .context("create new restore backup")?;
    serde_json::to_writer_pretty(
        &mut file,
        &json!({"format":"cranklink-setting-backup-v1","serial":serial,"slot":slot,"setting":s.name,"before":before,"original_parameters":hex::encode(&current),"planned_parameters":hex::encode(original)}),
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    let changed = current[..3] != original[..3];
    if changed {
        session
            .command(slot, 0x48, 0x50, original, 0x52)
            .context("date restore outcome uncertain; backup retained")?;
        let got = session.command(slot, 0x48, 0x54, &[0], 0x56)?;
        ensure!(
            got.get(..3) == Some(&original[..3]),
            "date restore readback mismatch"
        );
    }
    Ok(ChangeReceipt {
        setting: s.name.clone(),
        before,
        after,
        backup: path.display().to_string(),
        changed,
    })
}

pub(crate) fn validate_input(s: &SettingSpec, value: &str) -> Result<()> {
    ensure!(!s.read_only, "setting {} is read-only", s.name);
    if s.value_type == "date" {
        parse_date(value)?;
    } else if s.width == 0 {
        ensure!(
            value.is_ascii() && value.len() <= 10 && value.bytes().all(|b| (32..=126).contains(&b)),
            "profile name requires at most 10 printable ASCII characters"
        );
    } else if value == "unset" {
        ensure!(s.nullable, "this setting does not support unset");
    } else {
        s.raw_value(value)?;
    }
    Ok(())
}

/// A component write and its dry run must only accept changes whose captured
/// original value can later pass the same schema during restore.
pub(crate) fn validate_current_value(
    model: SettingsModel,
    spec: &SettingSpec,
    value: &Value,
) -> Result<()> {
    if matches!(model, SettingsModel::Component(_)) {
        let input = match value {
            Value::Null => "unset".to_owned(),
            Value::String(value) => value.clone(),
            value => value.to_string(),
        };
        validate_input(spec, &input).context("current component value is outside the writable schema; refusing a change whose backup cannot be restored")?;
    }
    Ok(())
}

/// A name spans two independently acknowledged five-byte SETs. Failure after
/// the first chunk can leave a partially changed name; retain the pre-write
/// backup and read both chunks before deciding how to recover. There is no
/// atomic name transaction or automatic replay after an uncertain outcome.
fn set_name(
    session: &mut dyn UnitCommands,
    slot: u8,
    s: &SettingSpec,
    value: &str,
    backup_path: &Path,
) -> Result<ChangeReceipt> {
    let original = read_raw(session, slot, s)?;
    let before = s.decode(&original)?;
    let after = json!(value);
    let serial = hex::encode(crate::device::serial_bytes(&session.command(
        slot,
        1,
        0x3c,
        &[0],
        0x3e,
    )?)?);
    let mut backup = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup_path)
        .context("create settings backup (choose a new filename)")?;
    serde_json::to_writer_pretty(
        &mut backup,
        &json!({"format":"cranklink-setting-backup-v1","serial":serial,"slot":slot,"setting":s.name,"before":before,"original_parameters":hex::encode(&original)}),
    )?;
    backup.write_all(b"\n")?;
    backup.sync_all()?;
    let changed = before != after;
    if changed {
        let mut bytes = [0u8; 10];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        for i in 0..2 {
            let mut p = vec![s.query[0] | i as u8];
            p.extend_from_slice(&bytes[i * 5..i * 5 + 5]);
            if s.name_reserved_byte {
                p.push(0);
            }
            session
                .command(slot, 0x17, 0x20, &p, 0x22)
                .context("name write failed, outcome uncertain; backup retained")?;
        }
        ensure!(
            s.decode(&read_raw(session, slot, s)?)? == after,
            "profile name readback mismatch"
        );
    }
    Ok(ChangeReceipt {
        setting: s.name.clone(),
        before,
        after,
        backup: backup_path.display().to_string(),
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn older_schemas_expose_only_common_fields_with_their_own_ranges_and_layouts() {
        for model in [
            SettingsModel::E8000,
            SettingsModel::E7000,
            SettingsModel::E6100,
            SettingsModel::E6000,
        ] {
            let specs = schema_for_model(model);
            assert_eq!(specs.len(), 3);
            let tire = spec_for_model("tire-circumference", model).unwrap();
            let angle = spec_for_model("mounting-angle", model).unwrap();
            let lamp = spec_for_model("lamp", model).unwrap();
            assert_eq!(angle.template.len(), 2);
            assert_eq!(tire.template, [0, 0, 0, 0]);
            assert_eq!(lamp.choices, [0, 1]);
            assert!(lamp.raw_value("2").is_err());
            assert!(angle.raw_value("0").is_ok());
            if model == SettingsModel::E6000 {
                assert!(tire.raw_value("1999").is_err());
                assert!(tire.raw_value("2000").is_ok());
                assert!(tire.raw_value("2400").is_ok());
                assert!(tire.raw_value("2401").is_err());
                assert!(angle.raw_value("46").is_err());
            } else {
                assert!(tire.raw_value("1300").is_ok());
                assert!(tire.raw_value("3000").is_ok());
                assert!(angle.raw_value("60").is_ok());
                assert!(angle.raw_value("61").is_err());
            }
            assert!(spec_for_model("destination", model).is_err());
            assert!(spec_for_model("profile1.boost.torque", model).is_err());
        }
        let e5100 = schema_for_model(SettingsModel::E5100);
        assert!(e5100.iter().any(|s| s.name == "tire-circumference"));
        assert!(e5100.iter().any(|s| s.name == "speed-limit"));
        assert!(e5100
            .iter()
            .all(|s| !s.name.starts_with("profile") && s.name != "assist-pattern"));
        assert_eq!(
            spec_for_model("mounting-angle", SettingsModel::E5100)
                .unwrap()
                .template
                .len(),
            2
        );
    }
    #[test]
    fn decoded_snapshot_comparison_requires_all_values_and_ignores_reply_padding() {
        let before = serde_json::json!({"unavailable":{},"values":{"assist":{"value":5,"raw":"050000004d33ff"}}});
        let after = serde_json::json!({"unavailable":{},"values":{"assist":{"value":5,"raw":"05ffffffffffff"}}});
        assert!(decoded_snapshots_equal(&before, &after));
        for invalid in [
            serde_json::json!({"unavailable":{},"values":{"assist":{"value":4}}}),
            serde_json::json!({"unavailable":{},"values":{}}),
            serde_json::json!({"unavailable":{"assist":"timeout"},"values":{}}),
            serde_json::json!({"unavailable":{},"values":{"assist":{"raw":"05"}}}),
        ] {
            assert!(!decoded_snapshots_equal(&before, &invalid));
        }
    }
    #[test]
    fn modern_basic_character_ranges_and_legacy_schema_are_distinct() {
        for model in [
            SettingsModel::Ep600,
            SettingsModel::Ep801,
            SettingsModel::Ep500,
        ] {
            assert!(spec_for_model("profile1.trail.character", model)
                .unwrap()
                .raw_value("14")
                .is_ok());
            assert!(spec_for_model("profile1.eco.character", model)
                .unwrap()
                .raw_value("10")
                .is_err());
            assert!(spec_for_model("profile1.boost.character", model)
                .unwrap()
                .raw_value("3")
                .is_err());
            assert!(spec_for_model("profile1.boost.character", model)
                .unwrap()
                .raw_value("4")
                .is_ok());
            assert!(spec_for_model("torque-maximum", model).is_err());
            assert!(spec_for_model("max-power.boost", model).is_err());
            assert!(spec_for_model("profile1.fine-tune.character", model).is_err());
        }
        assert!(spec("profile1.trail.character")
            .unwrap()
            .raw_value("14")
            .is_err());
        assert_eq!(
            schema().len(),
            schema_for_model(SettingsModel::Ep800Crg).len()
        );
    }
    #[test]
    fn modern_torque_uses_vendor_levels_and_mode_ranges_in_nm() {
        assert_eq!(
            modern_torque_levels(20, 85).unwrap(),
            [20, 27, 34, 42, 49, 56, 63, 70, 78, 85]
        );
        assert_eq!(
            modern_torque_levels(20, 50).unwrap(),
            [20, 24, 27, 30, 33, 36, 40, 43, 46, 50]
        );
        assert!(validate_modern_torque(85, 3, 20, 85).is_ok());
        assert!(validate_modern_torque(86, 3, 20, 85).is_err());
        assert!(validate_modern_torque(85, 1, 20, 85).is_err());
        assert!(validate_modern_torque(70, 1, 20, 85).is_ok());
        assert!(validate_modern_torque(27, 3, 20, 85).is_err());
        assert!(validate_modern_torque(34, 3, 20, 85).is_ok());
        assert!(modern_torque_levels(20, 100).is_err());
        assert_eq!(
            modern_torque_levels(25, 80).unwrap(),
            [25, 31, 37, 43, 49, 56, 62, 68, 74, 80]
        );
    }
    #[test]
    fn modern_profile_edit_preserves_carry_cutoff_and_reconstructs_reserved_byte() {
        let modern = spec_for_model("profile1.trail.character", SettingsModel::Ep801).unwrap();
        let original = [2, 2, 85, 5, 3, 0x21, 0xff];
        assert!(validate_model_reply(&modern, &original).is_ok());
        let payload = updated_payload(&modern, &original, 14, false).unwrap();
        assert_eq!(payload, [2, 2, 85, 14, 3, 0x21, 0]);
        assert!(validate_model_reply(&modern, &[0x12, 2, 85, 5, 3, 0x21, 0]).is_err());
        assert!(validate_model_reply(&modern, &original[..5]).is_err());
        let legacy = spec("profile1.trail.character").unwrap();
        assert_eq!(
            updated_payload(&legacy, &original, 8, false).unwrap(),
            [2, 2, 85, 8, 3, 0, 0]
        );
        let display = spec_for_model("profile1.display", SettingsModel::Ep600).unwrap();
        assert_eq!(
            updated_payload(&display, &[2, 0, 0xff, 0xff], 1, false).unwrap(),
            [2, 1]
        );
        assert_eq!(
            updated_payload(
                &spec("profile1.display").unwrap(),
                &[2, 0, 0xff, 0xff],
                1,
                false
            )
            .unwrap(),
            [2, 1, 0, 0]
        );
    }
    #[test]
    fn modern_profile_name_uses_exact_six_byte_mobile_chunks_without_changing_legacy() {
        struct Motor {
            name: [u8; 10],
            expected_parameters: usize,
            backup: std::path::PathBuf,
            sets: usize,
        }
        impl UnitCommands for Motor {
            fn command(
                &mut self,
                _: u8,
                group: u8,
                command: u8,
                params: &[u8],
                _: u8,
            ) -> Result<Vec<u8>> {
                Ok(match (group, command) {
                    (1, 0x3c) => vec![1, 2, 3, 4, 5, 6],
                    (0x17, 0x24) => {
                        let start = (params[0] & 1) as usize * 5;
                        [vec![params[0]], self.name[start..start + 5].to_vec()].concat()
                    }
                    (0x17, 0x20) => {
                        assert!(self.backup.exists());
                        assert_eq!(params.len(), self.expected_parameters);
                        assert!([0x50, 0x51].contains(&params[0]));
                        let start = (params[0] & 1) as usize * 5;
                        self.name[start..start + 5].copy_from_slice(&params[1..6]);
                        self.sets += 1;
                        vec![]
                    }
                    _ => panic!("unexpected command {group:02x}/{command:02x}"),
                })
            }
        }
        for (index, model, expected_parameters) in [
            (0, SettingsModel::Ep800Crg, 7),
            (1, SettingsModel::Ep600, 6),
            (2, SettingsModel::Ep801, 6),
        ] {
            let backup = std::env::temp_dir().join(format!(
                "cranklink-name-layout-{}-{}-{index}.json",
                std::process::id(),
                crate::platform::now_ms()
            ));
            let mut motor = Motor {
                name: *b"BeforeName",
                expected_parameters,
                backup: backup.clone(),
                sets: 0,
            };
            let receipt =
                set_for_model(&mut motor, 13, "profile1.name", "NewName", &backup, model).unwrap();
            assert!(receipt.changed);
            assert_eq!(receipt.after, json!("NewName"));
            assert_eq!(motor.sets, 2);
            assert_eq!(motor.name, *b"NewName\0\0\0");
            std::fs::remove_file(backup).unwrap();
        }
    }
    #[test]
    fn modern_destination_speed_ranges_do_not_change_ep800_limits() {
        assert!(validate_modern_speed_destination(1400, 2).is_ok());
        assert!(validate_modern_speed_destination(2500, 2).is_err());
        assert!(validate_modern_speed_destination(4500, 5).is_ok());
        assert!(validate_modern_speed_destination(4600, 5).is_err());
        assert!(validate_modern_speed_destination(2600, 0).is_err());
        assert!(validate_speed_destination(4500, 5).is_err());
        assert!(validate_speed_destination(1400, 2).is_err());
    }
    #[test]
    fn modern_change_and_serial_bound_restore_preserve_extended_profile_fields() {
        struct Motor {
            profile: Vec<u8>,
            original_backup: std::path::PathBuf,
            sets: usize,
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
                Ok(match (group, command) {
                    (1, 0x3c) => vec![1, 2, 3, 4, 5, 6, 0xff],
                    (0x16, 0xfc) => {
                        assert_eq!(params, [2, 3]);
                        self.profile.clone()
                    }
                    (0x0b, 0x84) => vec![0, 12],
                    (0x17, 0x1c) => vec![20, 85, 0xff, 0xff],
                    (0x16, 0xf8) => {
                        assert!(self.original_backup.exists(), "durable backup precedes SET");
                        let backup: Value =
                            serde_json::from_slice(&std::fs::read(&self.original_backup)?)?;
                        assert_eq!(
                            backup["serial"],
                            if self.sets == 0 {
                                "010203040506"
                            } else {
                                "010203040506ff"
                            }
                        );
                        assert_eq!(
                            params[5], 0x21,
                            "carry-over/cut-off must survive both edits"
                        );
                        assert_eq!(params[6], 0, "reserved byte is reconstructed");
                        self.profile = params.to_vec();
                        self.sets += 1;
                        vec![]
                    }
                    _ => panic!("unexpected command {group:02x}/{command:02x}"),
                })
            }
        }
        let prefix = format!(
            "cranklink-modern-profile-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        );
        let original = std::env::temp_dir().join(format!("{prefix}-original.json"));
        let current = std::env::temp_dir().join(format!("{prefix}-current.json"));
        let mut motor = Motor {
            profile: vec![2, 3, 70, 6, 3, 0x21, 0xff],
            original_backup: original.clone(),
            sets: 0,
        };
        let changed = set_for_model(
            &mut motor,
            13,
            "profile1.boost.torque",
            "85",
            &original,
            SettingsModel::Ep801,
        )
        .unwrap();
        assert_eq!(changed.before, json!(70));
        assert_eq!(changed.after, json!(85));
        assert!(changed.changed);
        // Backups created before serial normalization included reserved padding.
        let mut legacy_backup: Value =
            serde_json::from_slice(&std::fs::read(&original).unwrap()).unwrap();
        legacy_backup["serial"] = json!("010203040506ff");
        std::fs::write(&original, serde_json::to_vec(&legacy_backup).unwrap()).unwrap();
        let restored =
            restore_for_model(&mut motor, 13, &original, &current, SettingsModel::Ep801).unwrap();
        assert_eq!(restored.after, json!(70));
        assert_eq!(motor.sets, 2);
        let restore_backup: Value =
            serde_json::from_slice(&std::fs::read(&current).unwrap()).unwrap();
        assert_eq!(restore_backup["serial"], "010203040506");
        assert_eq!(motor.profile, [2, 3, 70, 6, 3, 0x21, 0]);
        std::fs::remove_file(original).unwrap();
        std::fs::remove_file(current).unwrap();
    }
    #[test]
    fn factory_values_and_unset_decode() {
        let destination = spec("destination").unwrap();
        assert_eq!(destination.query, [1]);
        assert_eq!(destination.decode(&[1, 3, 255, 255]).unwrap(), json!(3));
        assert_eq!(parse_date("2024-02-29").unwrap(), [24, 2, 29, 0]);
        assert!(parse_date("2025-02-29").is_err());
        assert_eq!(parse_date("unset").unwrap(), [255; 4]);
        assert_eq!(
            spec("maintenance-date").unwrap().decode(&[255; 4]).unwrap(),
            Value::Null
        );
        let character = spec("profile1.eco.character").unwrap();
        assert_eq!(
            updated_payload(&character, &[2, 1, 27, 1, 4, 255, 255], 2, false).unwrap(),
            [2, 1, 27, 2, 4, 0, 0]
        );
        let torque = spec("profile1.eco.torque").unwrap();
        assert_eq!(torque.raw_value("27").unwrap(), 27);
        assert_eq!(
            torque.decode(&[2, 1, 27, 1, 4, 255, 255]).unwrap(),
            json!(27)
        );
        assert_eq!(
            spec("maintenance-distance")
                .unwrap()
                .decode(&[255; 4])
                .unwrap(),
            Value::Null
        );
        assert_eq!(
            spec("maintenance-year").unwrap().decode(&[255; 4]).unwrap(),
            Value::Null
        );
        assert!(validate_combination(&spec("maintenance-day").unwrap(), &[255; 4]).is_ok());
    }
    #[test]
    fn profiles_use_user_selectors() {
        let s = spec("profile1.eco.character").unwrap();
        assert_eq!(s.query, [2, 1]);
        assert_eq!(s.set, 248);
        assert_eq!(spec("profile2.boost.start").unwrap().query, [3, 3]);
    }
    #[test]
    fn modern_destination_uses_factory_only_when_oem_is_unset_and_never_defaults_on_error() {
        struct Destination {
            replies: std::collections::VecDeque<Vec<u8>>,
            queries: Vec<u8>,
        }
        impl UnitCommands for Destination {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                response: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!((slot, group, command, response), (0, 0x16, 0xac, 0xae));
                self.queries.push(params[0]);
                self.replies
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("destination read failed"))
            }
        }
        for (replies, expected, queries) in [
            (vec![vec![1, 1]], Some(1), vec![1]),
            (vec![vec![1, 255], vec![0, 0]], Some(0), vec![1, 0]),
            (vec![vec![1, 255], vec![0, 1]], Some(1), vec![1, 0]),
            (vec![vec![1, 255], vec![0, 255]], None, vec![1, 0]),
            (vec![vec![1]], None, vec![1]),
            (vec![vec![0, 1]], None, vec![1]),
            (vec![], None, vec![1]),
        ] {
            let mut motor = Destination {
                replies: replies.into(),
                queries: vec![],
            };
            let result = effective_destination(&mut motor, 0, SettingsModel::Ep600);
            assert_eq!(result.ok(), expected);
            assert_eq!(motor.queries, queries);
            if let Some(region) = expected {
                assert_eq!(
                    validate_speed_destination(2800, region).is_ok(),
                    region == 1
                );
            }
        }
    }
    #[test]
    fn legacy_unset_destination_never_sends_unsupported_factory_read_or_setting_write() {
        struct Legacy;
        impl UnitCommands for Legacy {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                reply: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!(
                    (slot, group, command, params, reply),
                    (13, 0x16, 0xac, &[1][..], 0xae)
                );
                Ok(vec![1, 255, 0, 0])
            }
        }
        let err = effective_destination(&mut Legacy, 13, SettingsModel::Ep800Crg).unwrap_err();
        assert!(err
            .to_string()
            .contains("factory destination GET selector 0 is unsupported"));
    }
    #[test]
    fn speed_units_and_limits() {
        let s = spec("speed-limit").unwrap();
        assert_eq!(s.raw_value("25.00").unwrap(), 2500);
        // After reducing speed to 24, destination still permits serial-bound restoration to 25.
        assert!(validate_speed_destination(2500, 0).is_ok());
        assert!(validate_speed_destination(2600, 0).is_err());
        assert!(validate_speed_destination(3200, 1).is_ok());
        assert!(schema().iter().all(|x| x.name != "speed-limit-oem"));
        assert_eq!(s.decode(&[0xc4, 9, 255, 255]).unwrap(), json!(25.0));
        assert!(s.raw_value("25.001").is_err());
    }
    #[test]
    fn signed_and_ranges() {
        let s = spec("display-speed-adjustment").unwrap();
        assert_eq!(s.decode(&[206]).unwrap(), json!(-50));
        assert!(s.raw_value("-49").is_err());
        assert!(spec("profile1.eco.character")
            .unwrap()
            .raw_value("10")
            .is_err());
    }
    #[test]
    fn committed_web_schemas_match_the_rust_tables() {
        // The web app prefers these static files over the WASM schema; regenerate
        // them with `node --experimental-strip-types tools/sync-schemas.ts` after changing a table.
        macro_rules! schemas {
            ($($slug:literal),*) => {
                [$(($slug, include_str!(concat!("../web-instrument/schemas/", $slug, ".json")))),*]
            };
        }
        for (slug, committed) in schemas!(
            "ep800-crg",
            "ep600",
            "ep801",
            "ep500",
            "e5100",
            "e8000",
            "e7000",
            "e6100",
            "e6000"
        ) {
            let model = SettingsModel::from_name(slug).unwrap();
            assert_eq!(model.slug(), slug);
            let committed: Value = serde_json::from_str(committed).unwrap();
            assert_eq!(
                serde_json::to_value(schema_for_model(model)).unwrap(),
                committed,
                "web-instrument/schemas/{slug}.json is stale; run node --experimental-strip-types tools/sync-schemas.ts"
            );
        }
    }
    #[test]
    fn unsetting_any_calendar_component_clears_the_date() {
        struct Calendar {
            date: Vec<u8>,
            reserved_reads_zero: bool,
        }
        impl UnitCommands for Calendar {
            fn command(
                &mut self,
                _: u8,
                group: u8,
                command: u8,
                params: &[u8],
                _: u8,
            ) -> Result<Vec<u8>> {
                Ok(match (group, command) {
                    (1, 0x3c) => vec![1, 2, 3, 4, 5, 6],
                    (0x48, 0x50) => {
                        self.date = params.to_vec();
                        if self.reserved_reads_zero {
                            self.date[3] = 0;
                        }
                        vec![]
                    }
                    (0x48, _) => self.date.clone(),
                    _ => panic!("unexpected command {group:02x}/{command:02x}"),
                })
            }
        }
        for (case, (name, reserved_reads_zero)) in [
            ("maintenance-year", false),
            ("maintenance-month", false),
            ("maintenance-day", false),
            // Byte 3 is not part of the date; firmware need not echo it as FF.
            ("maintenance-day", true),
        ]
        .into_iter()
        .enumerate()
        {
            let mut calendar = Calendar {
                date: vec![24, 5, 10, 0],
                reserved_reads_zero,
            };
            let backup = std::env::temp_dir().join(format!(
                "cranklink-calendar-{case}-{name}-{}-{}.json",
                std::process::id(),
                crate::platform::now_ms()
            ));
            let receipt = set_for_model(
                &mut calendar,
                13,
                name,
                "unset",
                &backup,
                SettingsModel::Ep800Crg,
            )
            .unwrap();
            assert_eq!(receipt.after, Value::Null, "{name}");
            assert_eq!(calendar.date[..3], [255; 3], "{name}");
            std::fs::remove_file(backup).unwrap();
        }
    }
    #[test]
    fn date_validated() {
        let s = spec("maintenance-day").unwrap();
        assert!(validate_combination(&s, &[24, 2, 29, 0]).is_ok());
        assert!(validate_combination(&s, &[25, 2, 29, 0]).is_err());
    }
}
