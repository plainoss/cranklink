//! Settings and authorization capabilities selected from live motor identity.
//! Patch allowlists are unnecessary; older families do have authorization
//! introduction versions, which must be honored before sending any keys.
use crate::settings::SettingsModel;
use anyhow::{bail, ensure, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthProtocol {
    NotRequired,
    Legacy,
    Modern,
}

#[derive(Debug, Clone, Copy)]
pub struct MotorSupport {
    pub auth: AuthProtocol,
    pub settings: Option<SettingsModel>,
}

/// Match exact known seven-byte product attributes, including their padding.
pub fn for_motor_params(series: u8, unit: u8, params: &[u8]) -> Result<MotorSupport> {
    ensure!(unit == 0, "unsupported motor identity");
    if matches!(series, 0x08 | 0x0a) {
        ensure!(params.is_empty(), "invalid motor product attributes");
        return Ok(MotorSupport {
            auth: AuthProtocol::NotRequired,
            settings: Some(if series == 0x08 {
                SettingsModel::E6000
            } else {
                SettingsModel::E6001
            }),
        });
    }
    ensure!(params.len() == 7, "invalid motor product attributes");
    let (auth, settings) = match (series, params) {
        (0x23, b"00\0\0\0\0\0" | b"00-CRG\0") => (AuthProtocol::Legacy, SettingsModel::Ep800Crg),
        (0x30, b"00\0\0\0\0\0" | b"00-CRG\0") => (AuthProtocol::Modern, SettingsModel::Ep600),
        (0x35, b"01\0\0\0\0\0" | b"01-CRG\0") => (AuthProtocol::Modern, SettingsModel::Ep801),
        (0x37, b"00\0\0\0\0\0") => (AuthProtocol::Modern, SettingsModel::Ep500),
        (0x38, [0x10, 0, 0, 0, 0, 0, 0]) => (AuthProtocol::Legacy, SettingsModel::E5100),
        (0x12, [0 | 8, 0, 0, 0, 0, 0, 0]) => (AuthProtocol::Legacy, SettingsModel::E8000),
        (0x21, [0, 0, 0, 0, 0, 0, 0]) => (AuthProtocol::Legacy, SettingsModel::E7000),
        (0x20, [0x10 | 0x11 | 0x18, 0, 0, 0, 0, 0, 0])
        | (0x20, [0x10 | 0x18, 1, 0, 0, 0, 0, 0]) => (AuthProtocol::Legacy, SettingsModel::E6100),
        (0x0c, [0, 0x10, 0, 0, 0, 0, 0] | [0x01 | 0x05, 0, 0, 0, 0, 0, 0]) => {
            (AuthProtocol::NotRequired, SettingsModel::E6001)
        }
        (0x15, [0 | 1, 0x20, 0, 0, 0, 0, 0]) => (AuthProtocol::Legacy, SettingsModel::E6002),
        _ => bail!("settings/authentication is unsupported for this motor identity"),
    };
    Ok(MotorSupport {
        auth,
        settings: Some(settings),
    })
}

/// These stock identities have no supported product discriminator query.
/// Do not send an optional opcode merely to select its common schema.
pub fn read_params(
    session: &mut dyn crate::device::UnitCommands,
    slot: u8,
    series: u8,
) -> Result<Vec<u8>> {
    if matches!(series, 0x08 | 0x0a) {
        return Ok(Vec::new());
    }
    session.command(slot, 0x16, 0x7c, &[0], 0x7e)
}

pub(crate) fn authorization_for_motor(
    series: u8,
    unit: u8,
    params: &[u8],
    firmware: &str,
) -> Result<AuthProtocol> {
    let support = for_motor_params(series, unit, params)?;
    let version = firmware_version(firmware)?;
    let minimum = match series {
        0x15 => Some([3, 4, 0, 0]),
        0x12 => Some([4, 7, 0, 0]),
        0x21 => Some([4, 4, 0, 0]),
        0x20 if params[1] == 0 => Some([4, 4, 0, 0]),
        _ => None,
    };
    Ok(if minimum.is_some_and(|minimum| version < minimum) {
        AuthProtocol::NotRequired
    } else {
        support.auth
    })
}

/// Select the live motor's settings model, including firmware-dependent fields.
pub fn settings_for_motor(
    series: u8,
    unit: u8,
    params: &[u8],
    firmware: &str,
) -> Result<SettingsModel> {
    let model = for_motor_params(series, unit, params)?
        .settings
        .ok_or_else(|| anyhow::anyhow!("named settings are not implemented for this motor"))?;
    let version = firmware_version(firmware)?;
    Ok(if model == SettingsModel::E6002 && version < [3, 3, 0, 0] {
        SettingsModel::E6002Pre33
    } else {
        model
    })
}

fn firmware_version(firmware: &str) -> Result<[u16; 4]> {
    let components = firmware
        .split('.')
        .map(str::parse::<u16>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(components.len() == 4, "invalid motor firmware version");
    Ok([components[0], components[1], components[2], components[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_families_select_authorization_at_its_introduction_version() {
        for (series, params, before, introduced) in [
            (0x12, [0; 7], "4.6.99.0", "4.7.0.0"),
            (0x12, [8, 0, 0, 0, 0, 0, 0], "4.6.99.0", "4.7.0.0"),
            (0x21, [0; 7], "4.3.99.0", "4.4.0.0"),
            (0x20, [0x10, 0, 0, 0, 0, 0, 0], "4.3.99.0", "4.4.0.0"),
            (0x20, [0x11, 0, 0, 0, 0, 0, 0], "4.3.99.0", "4.4.0.0"),
            (0x20, [0x18, 0, 0, 0, 0, 0, 0], "4.3.99.0", "4.4.0.0"),
        ] {
            assert_eq!(
                authorization_for_motor(series, 0, &params, before).unwrap(),
                AuthProtocol::NotRequired
            );
            assert_eq!(
                authorization_for_motor(series, 0, &params, introduced).unwrap(),
                AuthProtocol::Legacy
            );
            assert_eq!(
                authorization_for_motor(series, 0, &params, "5.0.0.0").unwrap(),
                AuthProtocol::Legacy
            );
        }
        for params in [[0x10, 1, 0, 0, 0, 0, 0], [0x18, 1, 0, 0, 0, 0, 0]] {
            assert_eq!(
                authorization_for_motor(0x20, 0, &params, "4.0.0.0").unwrap(),
                AuthProtocol::Legacy
            );
        }
        assert_eq!(
            authorization_for_motor(0x38, 0, &[0x10, 0, 0, 0, 0, 0, 0], "4.1.0.0").unwrap(),
            AuthProtocol::Legacy
        );
    }

    #[test]
    fn e6000_generations_have_distinct_identities_and_authorization() {
        for (series, params, expected) in [
            (0x08, &[][..], SettingsModel::E6000),
            (0x0a, &[][..], SettingsModel::E6001),
            (0x0c, &[0, 0x10, 0, 0, 0, 0, 0][..], SettingsModel::E6001),
            (0x0c, &[1, 0, 0, 0, 0, 0, 0][..], SettingsModel::E6001),
            (0x0c, &[5, 0, 0, 0, 0, 0, 0][..], SettingsModel::E6001),
        ] {
            for version in ["3.2.0.0", "3.4.0.0", "3.9.0.0"] {
                assert_eq!(
                    settings_for_motor(series, 0, params, version).unwrap(),
                    expected
                );
                assert_eq!(
                    authorization_for_motor(series, 0, params, version).unwrap(),
                    AuthProtocol::NotRequired
                );
            }
        }
        for params in [[0, 0x20, 0, 0, 0, 0, 0], [1, 0x20, 0, 0, 0, 0, 0]] {
            assert_eq!(
                settings_for_motor(0x15, 0, &params, "3.2.99.0").unwrap(),
                SettingsModel::E6002Pre33
            );
            assert_eq!(
                settings_for_motor(0x15, 0, &params, "3.3.0.0").unwrap(),
                SettingsModel::E6002
            );
            assert_eq!(
                authorization_for_motor(0x15, 0, &params, "3.3.99.0").unwrap(),
                AuthProtocol::NotRequired
            );
            assert_eq!(
                authorization_for_motor(0x15, 0, &params, "3.4.0.0").unwrap(),
                AuthProtocol::Legacy
            );
        }
        for invalid in ["", "4.7", "4.7.0.x", "4.7.0.0.1"] {
            assert!(authorization_for_motor(0x12, 0, &[0; 7], invalid).is_err());
            assert!(settings_for_motor(0x15, 0, &[0, 0x20, 0, 0, 0, 0, 0], invalid).is_err());
        }
    }

    #[test]
    fn known_families_share_policy_across_standard_and_cargo_variants() {
        for (series, params, model, auth) in [
            (
                0x23,
                &b"00\0\0\0\0\0"[..],
                SettingsModel::Ep800Crg,
                AuthProtocol::Legacy,
            ),
            (
                0x23,
                &b"00-CRG\0"[..],
                SettingsModel::Ep800Crg,
                AuthProtocol::Legacy,
            ),
            (
                0x30,
                &b"00\0\0\0\0\0"[..],
                SettingsModel::Ep600,
                AuthProtocol::Modern,
            ),
            (
                0x30,
                &b"00-CRG\0"[..],
                SettingsModel::Ep600,
                AuthProtocol::Modern,
            ),
            (
                0x35,
                &b"01\0\0\0\0\0"[..],
                SettingsModel::Ep801,
                AuthProtocol::Modern,
            ),
            (
                0x35,
                &b"01-CRG\0"[..],
                SettingsModel::Ep801,
                AuthProtocol::Modern,
            ),
        ] {
            let support = for_motor_params(series, 0, params).unwrap();
            assert_eq!(support.settings, Some(model));
            assert_eq!(support.auth, auth);
        }
    }

    #[test]
    fn ep500_uses_its_named_modern_settings_schema() {
        let support = for_motor_params(0x37, 0, b"00\0\0\0\0\0").unwrap();
        assert_eq!(support.auth, AuthProtocol::Modern);
        assert_eq!(support.settings, Some(SettingsModel::Ep500));
    }

    #[test]
    fn rejects_other_units_unknown_variants_and_malformed_attributes() {
        for (series, unit, params) in [
            (0x23, 1, &b"00-CRG\0"[..]),
            (0x38, 0, &b"00\0\0\0\0\0"[..]),
            (0x35, 0, &b"00-CRG\0"[..]),
            (0x30, 0, &b"XX-CRG\0"[..]),
            (0x23, 0, &b"00-CRG"[..]),
            (0x23, 0, &b"00-CRG\0\0"[..]),
            (0x23, 0, &b"00\0X\0\0\0"[..]),
            (0x08, 0, &[0; 7][..]),
            (0x0a, 0, &[0; 7][..]),
            (0x0c, 0, &[0, 0x20, 0, 0, 0, 0, 0][..]),
            (0x15, 0, &[0, 0x10, 0, 0, 0, 0, 0][..]),
            (0x15, 1, &[0, 0x20, 0, 0, 0, 0, 0][..]),
        ] {
            assert!(for_motor_params(series, unit, params).is_err());
        }
    }
}
