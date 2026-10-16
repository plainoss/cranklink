//! Metadata-only selection. No serial probing or opening happens here.
use anyhow::{bail, Result};

#[derive(Clone, Debug)]
pub(super) struct SerialCandidate {
    pub port: String,
    pub serial: Option<String>,
}
#[derive(Clone, Debug)]
pub(super) struct UsbCandidate {
    pub identity: String,
    pub serial: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Serial(String),
    Usb(usize),
}

/// Each inventory contains only exact 1E44:7151 matches. A broken enumerator
/// doesn't discard a usable uniquely identified candidate from the other one.
pub(super) fn select(
    serial: Result<Vec<SerialCandidate>, String>,
    usb: Result<Vec<UsbCandidate>, String>,
    prefer_serial: bool,
) -> Result<Choice> {
    let serial_error = serial.as_ref().err().cloned();
    let usb_error = usb.as_ref().err().cloned();
    let serial = dedup_serial(serial.unwrap_or_default());
    let mut usb_unique: Vec<(usize, UsbCandidate)> = Vec::new();
    for (index, candidate) in usb.unwrap_or_default().into_iter().enumerate() {
        if !usb_unique
            .iter()
            .any(|(_, existing)| existing.identity == candidate.identity)
        {
            usb_unique.push((index, candidate));
        }
    }
    if serial.len() > 1 || usb_unique.len() > 1 {
        bail!("multiple SM-PCE02 adapters found ({} serial devices, {} physical USB devices); disconnect the extra adapters or specify the intended --port explicitly", serial.len(), usb_unique.len());
    }
    // One exact VID/PID match in each view describes one adapter unless their
    // available serial identities conflict. Empty serials supply no correlation
    // information and are not counted twice.
    if let (Some(s), Some((_, u))) = (serial.first(), usb_unique.first()) {
        if let (Some(a), Some(b)) = (nonempty(&s.serial), nonempty(&u.serial)) {
            if !a.eq_ignore_ascii_case(b) {
                bail!("SM-PCE02 serial/USB identities conflict; selection is uncertain; specify --port after checking connected devices");
            }
        }
    }
    if prefer_serial {
        if let Some(s) = serial.first() {
            return Ok(Choice::Serial(s.port.clone()));
        }
    }
    if let Some((index, _)) = usb_unique.first() {
        return Ok(Choice::Usb(*index));
    }
    if let Some(s) = serial.first() {
        return Ok(Choice::Serial(s.port.clone()));
    }
    let mut details = String::new();
    if let Some(error) = serial_error {
        details.push_str(&format!("; serial enumeration failed: {error}"));
    }
    if let Some(error) = usb_error {
        details.push_str(&format!("; USB enumeration failed: {error}"));
    }
    bail!("no SM-PCE02 1E44:7151 found{details}; connect the adapter, or specify its known --port explicitly. Windows: use the adapter's FTDI COM driver. Linux: grant USB access with a udev rule for 1e44:7151");
}
fn nonempty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|s| !s.is_empty())
}
fn alias_key(port: &str) -> String {
    // IOKit reports callout and dial-in nodes for the very same serial service.
    if let Some(suffix) = port
        .strip_prefix("/dev/cu.")
        .or_else(|| port.strip_prefix("/dev/tty."))
    {
        return format!("mac:{suffix}");
    }
    if port.to_ascii_uppercase().starts_with("COM") {
        return port.to_ascii_uppercase();
    }
    port.to_owned()
}
fn dedup_serial(candidates: Vec<SerialCandidate>) -> Vec<SerialCandidate> {
    let mut unique: Vec<SerialCandidate> = Vec::new();
    for candidate in candidates {
        if let Some(existing) = unique
            .iter_mut()
            .find(|p| alias_key(&p.port) == alias_key(&candidate.port))
        {
            // Alias records that disagree on a nonempty serial must remain
            // ambiguous rather than silently suppressing an identity conflict.
            if let (Some(a), Some(b)) = (nonempty(&existing.serial), nonempty(&candidate.serial)) {
                if !a.eq_ignore_ascii_case(b) {
                    unique.push(candidate);
                    continue;
                }
            }
            if candidate.port.starts_with("/dev/cu.") {
                *existing = candidate;
            }
        } else {
            unique.push(candidate);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;
    fn serial(port: &str, id: &str) -> SerialCandidate {
        SerialCandidate {
            port: port.into(),
            serial: Some(id.into()),
        }
    }
    fn usb(location: &str, id: &str) -> UsbCandidate {
        UsbCandidate {
            identity: location.into(),
            serial: Some(id.into()),
        }
    }
    #[test]
    fn windows_prefers_com_and_posix_usb_without_double_counting_one_adapter() {
        for prefer_serial in [false, true] {
            let choice = select(
                Ok(vec![serial("COM3", "")]),
                Ok(vec![usb("bus1:port2", "")]),
                prefer_serial,
            )
            .unwrap();
            assert_eq!(
                choice,
                if prefer_serial {
                    Choice::Serial("COM3".into())
                } else {
                    Choice::Usb(0)
                }
            );
        }
    }
    #[test]
    fn aliases_collapse_but_identical_serial_strings_do_not_hide_two_devices() {
        assert_eq!(
            select(
                Ok(vec![
                    serial("/dev/tty.usbserial-A", "A"),
                    serial("/dev/cu.usbserial-A", "A")
                ]),
                Ok(vec![]),
                false
            )
            .unwrap(),
            Choice::Serial("/dev/cu.usbserial-A".into())
        );
        assert!(select(
            Ok(vec![serial("COM3", "same"), serial("COM4", "same")]),
            Ok(vec![usb("bus1", "same")]),
            true
        )
        .is_err());
        assert!(select(
            Ok(vec![serial("COM3", "same")]),
            Ok(vec![usb("bus1", "same"), usb("bus2", "same")]),
            true
        )
        .is_err());
    }
    #[test]
    fn duplicate_usb_records_collapse_by_physical_id_and_conflicting_views_refuse() {
        assert_eq!(
            select(
                Ok(vec![]),
                Ok(vec![usb("bus1", "A"), usb("bus1", "A")]),
                false
            )
            .unwrap(),
            Choice::Usb(0)
        );
        assert!(select(
            Ok(vec![serial("COM3", "A")]),
            Ok(vec![usb("bus1", "B")]),
            true
        )
        .is_err());
    }
    #[test]
    fn independent_enumeration_errors_preserve_other_unique_backend_and_diagnostics() {
        assert_eq!(
            select(Err("serial denied".into()), Ok(vec![usb("bus1", "")]), true).unwrap(),
            Choice::Usb(0)
        );
        assert_eq!(
            select(
                Ok(vec![serial("COM3", "")]),
                Err("USB denied".into()),
                false
            )
            .unwrap(),
            Choice::Serial("COM3".into())
        );
        let error = select(Err("serial denied".into()), Err("USB denied".into()), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("serial denied") && error.contains("USB denied"));
        assert!(error.contains("FTDI COM") && error.contains("udev"));
    }
}
