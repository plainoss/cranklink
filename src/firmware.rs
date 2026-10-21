//! Offline firmware container validation. MD5 is an integrity check, not a signature.
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
use anyhow::{bail, ensure, Context, Result};
use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, path::Path};

const MARKER: [u8; 16] = [
    0x6f, 0x1b, 0x52, 0x9d, 0x84, 0x7c, 0x17, 0xdb, 0xd7, 0x2a, 0xf3, 0xd3, 0x37, 0xcb, 0x3c, 0xc3,
];
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version(pub [u8; 4]);
impl Version {
    pub fn packed(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() >= 3, "truncated packed version");
        let major = bytes[0] >> 4;
        let minor = bytes[0] & 15;
        ensure!(major <= 9 && minor <= 15, "invalid packed BCD version");
        Ok(Self([major, minor, bytes[1], bytes[2]]))
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirmwareKind {
    DcasX,
    Dcas2i,
    Dcas2,
    Renesas,
}

impl FirmwareKind {
    pub fn is_dcas(self) -> bool {
        matches!(self, Self::DcasX | Self::Dcas2i | Self::Dcas2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationTier {
    HardwareVerified,
    ProtocolVerified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotorFlashProfile {
    pub family_name: &'static str,
    pub live_series: u8,
    pub live_unit: u8,
    pub fw_series: u8,
    pub fw_unit: u8,
    pub dcas_kind: FirmwareKind,
    pub allowed_boards: &'static [u8],
    pub min_dcas_bootloader: Version,
    pub verification_tier: VerificationTier,
}

pub const MOTOR_FLASH_PROFILES: &[MotorFlashProfile] = &[
    MotorFlashProfile {
        family_name: "DU-EP800",
        live_series: 0x23,
        live_unit: 0x00,
        fw_series: 0x23,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::HardwareVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E6100",
        live_series: 0x20,
        live_unit: 0x00,
        fw_series: 0x20,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E7000",
        live_series: 0x21,
        live_unit: 0x00,
        fw_series: 0x20,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E5000",
        live_series: 0x22,
        live_unit: 0x00,
        fw_series: 0x22,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-EP600",
        live_series: 0x30,
        live_unit: 0x00,
        fw_series: 0x30,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-EP801",
        live_series: 0x35,
        live_unit: 0x00,
        fw_series: 0x30,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-EP500",
        live_series: 0x37,
        live_unit: 0x00,
        fw_series: 0x37,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E5100",
        live_series: 0x38,
        live_unit: 0x00,
        fw_series: 0x38,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::DcasX,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([5, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E8000",
        live_series: 0x12,
        live_unit: 0x00,
        fw_series: 0x12,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::Dcas2i,
        allowed_boards: &[1, 2],
        min_dcas_bootloader: Version([4, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E6002",
        live_series: 0x15,
        live_unit: 0x00,
        fw_series: 0x15,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::Dcas2i,
        allowed_boards: &[1, 2],
        min_dcas_bootloader: Version([4, 0, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E6001",
        live_series: 0x0a,
        live_unit: 0x00,
        fw_series: 0x0c,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::Dcas2,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([3, 2, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E6001",
        live_series: 0x0c,
        live_unit: 0x00,
        fw_series: 0x0c,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::Dcas2,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([3, 2, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
    MotorFlashProfile {
        family_name: "DU-E6000",
        live_series: 0x08,
        live_unit: 0x00,
        fw_series: 0x08,
        fw_unit: 0x00,
        dcas_kind: FirmwareKind::Dcas2,
        allowed_boards: &[1],
        min_dcas_bootloader: Version([3, 2, 0, 0]),
        verification_tier: VerificationTier::ProtocolVerified,
    },
];

pub fn find_motor_profile(
    live_series: u8,
    live_unit: u8,
    fw_series: u8,
    fw_unit: u8,
) -> Option<&'static MotorFlashProfile> {
    MOTOR_FLASH_PROFILES.iter().find(|p| {
        p.live_series == live_series
            && p.live_unit == live_unit
            && p.fw_series == fw_series
            && p.fw_unit == fw_unit
    })
}

pub fn is_supported_live_motor(live_series: u8, live_unit: u8) -> bool {
    MOTOR_FLASH_PROFILES
        .iter()
        .any(|p| p.live_series == live_series && p.live_unit == live_unit)
}

pub fn is_supported_rx_target(fw_series: u8, fw_unit: u8) -> bool {
    MOTOR_FLASH_PROFILES
        .iter()
        .any(|p| p.fw_series == fw_series && p.fw_unit == fw_unit)
}

pub fn is_allowed_motor_board(board: u8) -> bool {
    matches!(board, 1 | 2)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FirmwareMetadata {
    pub kind: FirmwareKind,
    pub version: Version,
    pub compatibility_version: Version,
    pub series: u8,
    pub unit: u8,
    pub board: Option<u8>,
    pub encrypted: bool,
    pub source_bytes: usize,
    pub decoded_bytes: usize,
    pub source_sha256: String,
    pub decoded_sha256: String,
    pub checksum: u8,
}

#[derive(Clone, Debug)]
pub struct Firmware {
    pub metadata: FirmwareMetadata,
    /// The complete plaintext image, including its MCU header and `et` trailer.
    pub data: Vec<u8>,
}

impl Firmware {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = crate::storage::read_bounded(path, MAX_IMAGE_BYTES)
            .with_context(|| format!("read {}", path.display()))?;
        Self::from_bytes(&raw)
    }

    pub fn from_bytes(raw: &[u8]) -> Result<Self> {
        ensure!(
            raw.len() <= MAX_IMAGE_BYTES,
            "firmware file exceeds 16 MiB limit"
        );
        let encrypted = raw.ends_with(&MARKER);
        let data = if encrypted {
            decode_container(raw)?
        } else {
            raw.to_vec()
        };
        ensure!(
            data.len() >= 64,
            "firmware header is truncated (minimum 64 bytes)"
        );
        ensure!(
            data.ends_with(b"et"),
            "firmware is missing the expected et trailer"
        );
        let (kind, version, compatibility_version, series, unit, board, declared) =
            if data[..16].iter().all(|b| *b == 0xff) {
                (
                    FirmwareKind::DcasX,
                    Version::packed(&data[16..19])?,
                    Version::packed(&data[47..50])?,
                    data[40],
                    data[41],
                    Some(data[43]),
                    u32::from_le_bytes(data[24..28].try_into().unwrap()) as usize,
                )
            } else if data.len() >= 0xd6
                && data[..4] == [0xff; 4]
                && data[7] == 0
                && data[11] == 0
                && data[15] == 0
                && data[0xc0] == 0x05
            {
                let kind = match data[0xcc] {
                    3 => FirmwareKind::Dcas2i,
                    1 | 2 => FirmwareKind::Dcas2,
                    other => bail!("unsupported legacy DCAS header version {other}"),
                };
                (
                    kind,
                    Version::packed(&data[0xc1..0xc4])?,
                    Version::packed(&data[0xd3..0xd6])?,
                    data[0xca],
                    data[0xcb],
                    Some(data[0xcd]),
                    u32::from_le_bytes(data[0xc6..0xca].try_into().unwrap()) as usize,
                )
            } else {
                // The recovered Renesas header pattern is deliberately narrow. Unknown
                // containers cannot become writable solely because their filename matches.
                const PATTERN: &[u8] = b"?0??0?00??????????0?0???????????";
                let hex = hex_string(&data[..15]);
                ensure!(
                    PATTERN
                        .iter()
                        .zip(hex.as_bytes())
                        .all(|(a, b)| *a == b'?' || a == b),
                    "unrecognized firmware header"
                );
                (
                    FirmwareKind::Renesas,
                    Version::packed(&data[8..11])?,
                    Version::packed(&data[16..19])?,
                    data[14],
                    data[15],
                    None,
                    u32::from_le_bytes(data[..4].try_into().unwrap()) as usize,
                )
            };
        ensure!(
            declared == data.len(),
            "firmware header size mismatch: declares {declared}, contains {} bytes",
            data.len()
        );
        let metadata = FirmwareMetadata {
            kind,
            version,
            compatibility_version,
            series,
            unit,
            board,
            encrypted,
            source_bytes: raw.len(),
            decoded_bytes: data.len(),
            source_sha256: hex_string(&Sha256::digest(raw)),
            decoded_sha256: hex_string(&Sha256::digest(&data)),
            checksum: data.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)),
        };
        Ok(Self { metadata, data })
    }

    pub fn validate_target(
        &self,
        series: u8,
        unit: u8,
        board: u8,
        bootloader: Version,
    ) -> Result<()> {
        self.validate_target_with_policy(series, unit, board, bootloader, false)
            .map(|_| ())
    }

    pub fn validate_target_with_policy(
        &self,
        series: u8,
        unit: u8,
        board: u8,
        bootloader: Version,
        allow_unverified_model: bool,
    ) -> Result<&'static MotorFlashProfile> {
        let m = &self.metadata;
        let profile = find_motor_profile(series, unit, m.series, m.unit).ok_or_else(|| {
            anyhow::anyhow!(
                "firmware identity {:02X}/{:02X} does not match target {series:02X}/{unit:02X} or is not a supported motor family",
                m.series,
                m.unit
            )
        })?;
        ensure!(
            profile.verification_tier == VerificationTier::HardwareVerified || allow_unverified_model,
            "model {} ({series:02X}/{unit:02X} -> fw {:02X}/{:02X}) is protocol-verified only; pass --allow-unverified-model to authorize flashing non-EP800 models",
            profile.family_name,
            m.series,
            m.unit
        );
        if m.kind.is_dcas() {
            ensure!(
                m.kind == profile.dcas_kind,
                "firmware kind {:?} does not match target profile {:?}",
                m.kind,
                profile.dcas_kind
            );
            if bootloader >= Version([3, 2, 4, 0]) {
                ensure!(
                    m.board == Some(board),
                    "firmware board {:?} does not match target board {board}",
                    m.board
                );
            }
            ensure!(
                profile.allowed_boards.contains(&board),
                "board {board} is not supported for {} (allowed: {:?})",
                profile.family_name,
                profile.allowed_boards
            );
            ensure!(
                bootloader >= profile.min_dcas_bootloader,
                "{} requires bootloader >= {}, found {bootloader}",
                profile.family_name,
                profile.min_dcas_bootloader
            );
            match m.kind {
                FirmwareKind::DcasX => {
                    ensure!(
                        !self.data.is_empty() && self.data.len().div_ceil(2048) <= 256,
                        "DCASX image exceeds the one-byte sector erase-code range"
                    );
                }
                FirmwareKind::Dcas2i => {
                    ensure!(
                        bootloader < Version([5, 0, 0, 0]),
                        "DCAS2i requires 4.x bootloader, found {bootloader}"
                    );
                    ensure!(
                        self.data.len() >= 0xd6 && self.data.len() <= 2 * 65536,
                        "DCAS2i image exceeds 128 KiB range"
                    );
                }
                FirmwareKind::Dcas2 => {
                    ensure!(
                        bootloader < Version([4, 0, 0, 0]),
                        "DCAS2 requires 3.x bootloader, found {bootloader}"
                    );
                    ensure!(
                        self.data.len() >= 0xd6 && self.data.len() <= 2 * 65536,
                        "DCAS2 image exceeds 128 KiB range"
                    );
                }
                FirmwareKind::Renesas => unreachable!(),
            }
        }
        Ok(profile)
    }

    /// Check a pair before programming either CPU. The D image advertises its
    /// required RX companion version; the M header does not require D >= M.
    pub fn validate_companion(&self, companion: &Self) -> Result<()> {
        ensure!(
            self.metadata.kind.is_dcas() != companion.metadata.kind.is_dcas(),
            "a firmware pair must contain one D and one M image"
        );
        ensure!(
            self.metadata.series == companion.metadata.series
                && self.metadata.unit == companion.metadata.unit,
            "companion firmware series/unit mismatch"
        );
        let (d, m) = if self.metadata.kind.is_dcas() {
            (self, companion)
        } else {
            (companion, self)
        };
        ensure!(
            d.metadata.compatibility_version == m.metadata.version || verified_public_pair(d, m),
            "D firmware {} requires RX {}, supplied RX {}",
            d.metadata.version,
            d.metadata.compatibility_version,
            m.metadata.version
        );
        Ok(())
    }

    pub fn validate_companion_version(&self, actual: Version) -> Result<()> {
        ensure!(
            self.metadata.kind.is_dcas(),
            "companion version check requires a D image"
        );
        ensure!(
            actual == self.metadata.compatibility_version,
            "D firmware requires RX {}, found {actual}",
            self.metadata.compatibility_version
        );
        Ok(())
    }

    pub fn validate_companion_version_for_pair(
        &self,
        companion: &Self,
        actual: Version,
    ) -> Result<()> {
        self.validate_companion(companion)?;
        ensure!(
            self.metadata.kind.is_dcas(),
            "companion version check requires a D image"
        );
        ensure!(
            actual == self.metadata.compatibility_version || actual == companion.metadata.version,
            "D firmware requires RX {} (or companion {}), found {actual}",
            self.metadata.compatibility_version,
            companion.metadata.version
        );
        Ok(())
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn common_key(md5: &[u8]) -> [u8; 16] {
    let a = u16::from_le_bytes([md5[0], md5[1]]) as u32;
    let b = u16::from_le_bytes([md5[2], md5[3]]) as u32;
    let c = u16::from_le_bytes([md5[4], md5[5]]) as u32;
    let mut state = [
        123456789 ^ ((a << 16) | b),
        362436069,
        521288629,
        88675123 ^ ((b << 16) | c),
    ];
    let step = |s: &mut [u32; 4]| {
        let t = s[0] ^ (s[0] << 11);
        *s = [s[1], s[2], s[3], s[3] ^ (s[3] >> 19) ^ t ^ (t >> 8)];
        s[3]
    };
    for _ in 0..(a >> 8) {
        step(&mut state);
    }
    let mut key = [0u8; 16];
    for chunk in key.chunks_exact_mut(4) {
        chunk.copy_from_slice(&step(&mut state).to_le_bytes());
    }
    key
}

fn decode_container(raw: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        raw.len() >= 68 && (raw.len() - 52).is_multiple_of(16),
        "invalid encrypted container length"
    );
    let footer = raw.len() - 36;
    let md5 = &raw[footer..footer + 16];
    let declared = u32::from_le_bytes(raw[footer + 16..footer + 20].try_into().unwrap()) as usize;
    ensure!(
        declared <= MAX_IMAGE_BYTES,
        "declared plaintext length exceeds 16 MiB limit"
    );
    let aes = aes::Aes128::new(GenericArray::from_slice(&common_key(md5)));
    let mut previous = raw[..16].to_vec();
    let mut plain = raw[16..footer].to_vec();
    for block in plain.chunks_exact_mut(16) {
        let encrypted = block.to_vec();
        aes.decrypt_block(GenericArray::from_mut_slice(block));
        for (byte, prev) in block.iter_mut().zip(&previous) {
            *byte ^= prev;
        }
        previous = encrypted;
    }
    let pad = *plain.last().context("empty encrypted payload")? as usize;
    ensure!(
        (1..=16).contains(&pad)
            && plain[plain.len() - pad..]
                .iter()
                .all(|b| *b as usize == pad),
        "invalid PKCS7 padding"
    );
    plain.truncate(plain.len() - pad);
    ensure!(plain.len() == declared, "declared plaintext size mismatch");
    if Md5::digest(&plain).as_slice() != md5 {
        bail!("plaintext MD5 mismatch");
    }
    Ok(plain)
}

// Current modern headers advertise a compatibility baseline rather than the
// exact companion release. Accept only the exact compatible image pairs;
// the separate target/board checks still prevent modern firmware programming.
fn verified_public_pair(d: &Firmware, m: &Firmware) -> bool {
    matches!(
        (
            d.metadata.decoded_sha256.as_str(),
            m.metadata.decoded_sha256.as_str()
        ),
        (
            "ccea58cacf7588b0d1a9c6037cde81f25510b4b2fbf59fdaf99b8099d16f3b80",
            "0596a5733075bc2270f4747e5fe22ee2a45613cb5330e070fab99295f516b413"
        ) | (
            "6dd6ebf14f04a2d363cced8f77a8c8f86b64583c71e2abb9b2cc0fe64e2d1952",
            "24722c252cafbe413c6ac4209ac7d9167a46c9e138bf718cf3317ed0d58ab32b"
        ) | (
            // 2DUE8000-D.4.10.0.dat + 2DUE8000-M.4.9.4.dat
            "0df812b21bf2b7f613e27e1e65de7dde60850284e3241c4d100a87b76bd17588",
            "d7a0257498838dec25ea7543a0b030dfaa6920b97cf5f0c8d00f7245938cb219"
        ) | (
            // DUE8000-D.4.10.0.dat + DUE8000-M.4.9.4.dat
            "50ee8cdfb65aca45e8ac1196626edd94eb2d5acf56f554f5c41eccc4189f86b3",
            "08505c29c87c834601d641d59b3b5cacfa51a3499e680f4df9c1585aa088a707"
        ) | (
            // DUE5000-D.4.5.0.dat + DUE5000-M.4.4.8.dat
            "44806bd54aedff95a88bb73fafe0f0581297f2f67b2ea35545cea012899d90bb",
            "9ea350e988345a9d9fb41c1363562ca190f1a8d5e1cc8a38c6373f69b877f033"
        ) | (
            // DUE5100-D.4.1.0.dat + DUE5100-M.4.0.0.dat
            "f3bcba12ac9685123fac3bc4c5198b8ae7da64753b8f8804bbfdbee87e0207fe",
            "661cbd606207eb135300ba60bc52436269414f6af79bb383922e355658e9d9b2"
        ) | (
            // DUE6100-D.4.8.0.dat + DUE6100-M.4.7.6.dat
            "47fa9c5d52d03987c919d623ee6cb9aae7182548084c51115cffbe7cb3dade7c",
            "598a9c53abce5a40531acdbb61b4ba2721a0fcc7352abac9dfbe0e7855e923df"
        ) | (
            // DUEP500-D.4.1.0.dat + DUEP500-M.4.1.0.dat
            "b273042b1f0e69aa35db227e5d590efc845acd6c40381e0ba0aab4d3e5a2333c",
            "14fd61ed41b013e2a39517aadeffca88ee8f05928f0ca69c56374fe690ecf7e9"
        ) | (
            // DUEP600-D.4.4.1.dat + DUEP600-M.4.4.0.dat
            "c2bdeb052b7f5d89190674346d1d4f3491e1a7eb05b3c79f17f86983e4fff6ac",
            "dba1ded5e2b8b94ae7c7353e68d16c0f467e2cd6e8a62f6c3ace7cb283661749"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;
    fn image(kind: FirmwareKind) -> Vec<u8> {
        let mut b = match kind {
            FirmwareKind::DcasX => {
                let mut b = vec![0xff; 128];
                b[16..19].copy_from_slice(&[0x42, 0, 0]);
                b[24..28].copy_from_slice(&128u32.to_le_bytes());
                b[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
                b[47..50].copy_from_slice(&[0x41, 11, 0]);
                b
            }
            FirmwareKind::Dcas2i | FirmwareKind::Dcas2 => {
                let mut b = vec![0xff; 256];
                b[4..16].copy_from_slice(&[0x01, 0, 0, 0, 0x02, 0, 0, 0, 0x03, 0, 0, 0]);
                b[0xc0] = 0x05;
                b[0xc1..0xc4].copy_from_slice(&[0x4a, 0, 0]);
                b[0xc6..0xca].copy_from_slice(&256u32.to_le_bytes());
                b[0xca..0xce].copy_from_slice(&[
                    0x12,
                    0,
                    if kind == FirmwareKind::Dcas2i { 3 } else { 2 },
                    1,
                ]);
                b[0xd3..0xd6].copy_from_slice(&[0x49, 0, 0]);
                b
            }
            FirmwareKind::Renesas => {
                let mut b = vec![0xff; 128];
                b[..4].copy_from_slice(&128u32.to_le_bytes());
                b[8..12].copy_from_slice(&[0x41, 11, 0, 1]);
                b[14..19].copy_from_slice(&[0x23, 0, 0x41, 11, 0]);
                b
            }
        };
        let len = b.len();
        b[len - 2..].copy_from_slice(b"et");
        b
    }
    fn container(plain: &[u8]) -> Vec<u8> {
        let md5 = Md5::digest(plain);
        let aes = aes::Aes128::new(GenericArray::from_slice(&common_key(&md5)));
        let mut result = vec![0x12; 16];
        let mut padded = plain.to_vec();
        let pad = 16 - padded.len() % 16;
        padded.extend(std::iter::repeat_n(pad as u8, pad));
        let mut prev = result.clone();
        for block in padded.chunks_exact_mut(16) {
            for (a, b) in block.iter_mut().zip(&prev) {
                *a ^= b;
            }
            aes.encrypt_block(GenericArray::from_mut_slice(block));
            result.extend_from_slice(block);
            prev = block.to_vec();
        }
        result.extend_from_slice(&md5);
        result.extend_from_slice(&(plain.len() as u32).to_le_bytes());
        result.extend_from_slice(&MARKER);
        result
    }
    #[test]
    fn decrypt_and_pair() {
        let d = Firmware::from_bytes(&container(&image(FirmwareKind::DcasX))).unwrap();
        let m = Firmware::from_bytes(&image(FirmwareKind::Renesas)).unwrap();
        assert_eq!(d.metadata.version.to_string(), "4.2.0.0");
        assert!(d.metadata.encrypted);
        assert_eq!(d.data, image(FirmwareKind::DcasX));
        d.validate_companion(&m).unwrap();
        m.validate_companion(&d).unwrap();
        d.validate_target(0x23, 0, 1, Version([5, 0, 6, 1]))
            .unwrap();
        assert!(d
            .validate_target(0x23, 0, 2, Version([5, 0, 6, 1]))
            .is_err());
        assert!(d
            .validate_target(0x23, 0, 1, Version([4, 9, 0, 0]))
            .is_err());
        assert!(d
            .validate_target(0x24, 0, 1, Version([5, 0, 6, 1]))
            .is_err());
        assert!(d.validate_companion_version(Version([4, 0, 2, 0])).is_err());
    }
    #[test]
    fn encrypted_renesas_uses_the_same_verified_container() {
        let plain = image(FirmwareKind::Renesas);
        let decoded = Firmware::from_bytes(&container(&plain)).unwrap();
        assert_eq!(decoded.data, plain);
        assert!(decoded.metadata.encrypted);
        assert_eq!(decoded.metadata.kind, FirmwareKind::Renesas);
    }
    #[test]
    fn current_public_stock_pairs_decode_and_keep_writers_fenced() {
        let Some(dir) = std::env::var_os("CRANKLINK_CURRENT_FIRMWARE_FIXTURES") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        for (dname, mname) in [
            ("DUEP600-D.4.6.0.dat", "DUEP600-M.4.4.2.dat"),
            ("DUEP500-D.4.1.1.dat", "DUEP500-M.4.1.1.dat"),
        ] {
            let d = Firmware::load(dir.join(dname)).unwrap();
            let m = Firmware::load(dir.join(mname)).unwrap();
            assert!(d.metadata.encrypted && m.metadata.encrypted);
            d.validate_companion(&m).unwrap();
            m.validate_companion(&d).unwrap();
            let mut altered = d.data.clone();
            altered[64] ^= 1;
            let altered = Firmware::from_bytes(&altered).unwrap();
            assert!(
                altered.validate_companion(&m).is_err(),
                "baseline version must not authorize an unknown image pair"
            );
            assert!(d
                .validate_target(d.metadata.series, d.metadata.unit, 1, Version([5, 0, 6, 1]))
                .is_err());
            assert!(m
                .validate_target(m.metadata.series, m.metadata.unit, 1, Version([5, 0, 6, 1]))
                .is_err());
        }
    }
    #[test]
    fn rejects_integrity_failures() {
        let good = container(&image(FirmwareKind::DcasX));
        let mut b = good.clone();
        b[40] ^= 1;
        assert!(Firmware::from_bytes(&b)
            .unwrap_err()
            .to_string()
            .contains("MD5 mismatch"));
        let mut b = good.clone();
        let n = b.len();
        b[n - 20] ^= 1;
        assert!(Firmware::from_bytes(&b)
            .unwrap_err()
            .to_string()
            .contains("size mismatch"));
        let mut b = good;
        b[15] ^= 1;
        assert!(Firmware::from_bytes(&b).is_err());
        assert!(Firmware::from_bytes(&MARKER).is_err());
    }
    #[test]
    fn rejects_headers_and_pairs() {
        let mut b = image(FirmwareKind::Renesas);
        b[8] = 0xfa;
        assert!(Firmware::from_bytes(&b).is_err());
        let mut b = image(FirmwareKind::DcasX);
        b[24] = 0;
        assert!(Firmware::from_bytes(&b).is_err());
        let d = Firmware::from_bytes(&image(FirmwareKind::DcasX)).unwrap();
        let mut b = image(FirmwareKind::Renesas);
        b[9] = 10;
        let m = Firmware::from_bytes(&b).unwrap();
        assert!(d.validate_companion(&m).is_err());
        assert!(d.validate_companion(&d).is_err());
        let mut b = image(FirmwareKind::DcasX);
        b[127] = 0;
        assert!(Firmware::from_bytes(&b).is_err());
    }
    /// Opt-in external fixture check; proprietary firmware is never embedded.
    #[test]
    fn external_vendor_fixtures() {
        let Some(dir) = std::env::var_os("CRANKLINK_FIRMWARE_FIXTURES") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let d = Firmware::load(dir.join("DUEP800-D.4.2.0.dat")).unwrap();
        let m = Firmware::load(dir.join("DUEP800-M.4.1.11.dat")).unwrap();
        assert_eq!(d.data.len(), 157996);
        assert_eq!(m.data.len(), 128192);
        assert_eq!(d.metadata.checksum, 0xcb);
        d.validate_companion(&m).unwrap();
    }
    #[test]
    fn dcas2i_decode_and_board_support() {
        let plain = image(FirmwareKind::Dcas2i);
        let fw = Firmware::from_bytes(&plain).unwrap();
        assert_eq!(fw.metadata.kind, FirmwareKind::Dcas2i);
        assert_eq!(fw.metadata.version, Version([4, 10, 0, 0]));
        assert_eq!(fw.metadata.version.to_string(), "4.10.0.0");
        assert_eq!(fw.metadata.compatibility_version, Version([4, 9, 0, 0]));
        assert_eq!(fw.metadata.series, 0x12);
        assert_eq!(fw.metadata.unit, 0);
        assert_eq!(fw.metadata.board, Some(1));

        // DU-E8000 is ProtocolVerified: allow_unverified_model = false must reject.
        assert!(fw
            .validate_target_with_policy(0x12, 0, 1, Version([4, 1, 0, 0]), false)
            .is_err());

        // With allow_unverified_model = true, Board 1 is accepted.
        let prof = fw
            .validate_target_with_policy(0x12, 0, 1, Version([4, 1, 0, 0]), true)
            .unwrap();
        assert_eq!(prof.family_name, "DU-E8000");

        // Board 2 is also accepted for DU-E8000.
        let mut b2 = plain.clone();
        b2[0xcd] = 2;
        let fw_b2 = Firmware::from_bytes(&b2).unwrap();
        assert_eq!(fw_b2.metadata.board, Some(2));
        let prof_b2 = fw_b2
            .validate_target_with_policy(0x12, 0, 2, Version([4, 1, 0, 0]), true)
            .unwrap();
        assert_eq!(prof_b2.family_name, "DU-E8000");

        // Board 3 is not supported for DU-E8000.
        assert!(fw_b2
            .validate_target_with_policy(0x12, 0, 3, Version([4, 1, 0, 0]), true)
            .is_err());

        // DCAS2i requires 4.x bootloader:
        assert!(fw
            .validate_target_with_policy(0x12, 0, 1, Version([3, 9, 0, 0]), true)
            .is_err());
        assert!(fw
            .validate_target_with_policy(0x12, 0, 1, Version([5, 0, 0, 0]), true)
            .is_err());
    }

    #[test]
    fn dcas2_decode_and_validation() {
        let mut plain = image(FirmwareKind::Dcas2);
        plain[0xca] = 0x08; // DU-E6000 series
        let fw = Firmware::from_bytes(&plain).unwrap();
        assert_eq!(fw.metadata.kind, FirmwareKind::Dcas2);
        assert_eq!(fw.metadata.version, Version([4, 10, 0, 0]));
        assert_eq!(fw.metadata.compatibility_version, Version([4, 9, 0, 0]));
        assert_eq!(fw.metadata.series, 0x08);

        // Protocol verified: false fails, true passes
        assert!(fw
            .validate_target_with_policy(0x08, 0, 1, Version([3, 2, 4, 0]), false)
            .is_err());
        let prof = fw
            .validate_target_with_policy(0x08, 0, 1, Version([3, 2, 4, 0]), true)
            .unwrap();
        assert_eq!(prof.family_name, "DU-E6000");

        // DCAS2 requires 3.x bootloader >= 3.2.0.0
        assert!(fw
            .validate_target_with_policy(0x08, 0, 1, Version([3, 1, 9, 0]), true)
            .is_err());
        assert!(fw
            .validate_target_with_policy(0x08, 0, 1, Version([4, 0, 0, 0]), true)
            .is_err());
    }

    #[test]
    fn validate_target_policy_and_aliases() {
        // EP800: HardwareVerified allows false
        let ep800_d = Firmware::from_bytes(&image(FirmwareKind::DcasX)).unwrap();
        assert!(ep800_d
            .validate_target_with_policy(0x23, 0, 1, Version([5, 0, 6, 1]), false)
            .is_ok());

        // EP801 alias: target live 0x35/00 -> fw 0x30/00
        let mut ep801_buf = image(FirmwareKind::DcasX);
        ep801_buf[40] = 0x30;
        let ep801_fw = Firmware::from_bytes(&ep801_buf).unwrap();
        assert!(ep801_fw
            .validate_target_with_policy(0x35, 0, 1, Version([5, 0, 0, 0]), false)
            .is_err());
        let p_ep801 = ep801_fw
            .validate_target_with_policy(0x35, 0, 1, Version([5, 0, 0, 0]), true)
            .unwrap();
        assert_eq!(p_ep801.family_name, "DU-EP801");

        // E7000 alias: target live 0x21/00 -> fw 0x20/00
        let mut e7000_buf = image(FirmwareKind::DcasX);
        e7000_buf[40] = 0x20;
        let e7000_fw = Firmware::from_bytes(&e7000_buf).unwrap();
        assert!(e7000_fw
            .validate_target_with_policy(0x21, 0, 1, Version([5, 0, 0, 0]), false)
            .is_err());
        let p_e7000 = e7000_fw
            .validate_target_with_policy(0x21, 0, 1, Version([5, 0, 0, 0]), true)
            .unwrap();
        assert_eq!(p_e7000.family_name, "DU-E7000");

        // E6001 alias: target live 0x0a/00 -> fw 0x0c/00 (DCAS2)
        let mut e6001_buf = image(FirmwareKind::Dcas2);
        e6001_buf[0xca] = 0x0c;
        let e6001_fw = Firmware::from_bytes(&e6001_buf).unwrap();
        assert!(e6001_fw
            .validate_target_with_policy(0x0a, 0, 1, Version([3, 2, 0, 0]), false)
            .is_err());
        let p_e6001 = e6001_fw
            .validate_target_with_policy(0x0a, 0, 1, Version([3, 2, 0, 0]), true)
            .unwrap();
        assert_eq!(p_e6001.family_name, "DU-E6001");
        // Also test direct live 0x0c/00 -> fw 0x0c/00
        assert!(e6001_fw
            .validate_target_with_policy(0x0c, 0, 1, Version([3, 2, 0, 0]), true)
            .is_ok());

        // Global helpers
        assert!(is_supported_live_motor(0x23, 0));
        assert!(is_supported_live_motor(0x35, 0));
        assert!(is_supported_live_motor(0x0a, 0));
        assert!(!is_supported_live_motor(0x99, 0));
        assert!(is_supported_rx_target(0x30, 0));
        assert!(is_supported_rx_target(0x0c, 0));
        assert!(!is_supported_rx_target(0x99, 0));
        assert!(is_allowed_motor_board(1));
        assert!(is_allowed_motor_board(2));
        assert!(!is_allowed_motor_board(3));
    }

    #[test]
    fn ep500_rejects_ep600_distribution_images() {
        // Live 37/00 is EP500 only; EP600/EP801 images (fw 30/00) must not pass.
        let mut ep600_buf = image(FirmwareKind::DcasX);
        ep600_buf[40] = 0x30;
        let ep600_fw = Firmware::from_bytes(&ep600_buf).unwrap();
        assert!(ep600_fw
            .validate_target_with_policy(0x37, 0, 1, Version([5, 0, 6, 1]), true)
            .is_err());
        assert!(find_motor_profile(0x37, 0, 0x30, 0).is_none());
    }

    #[test]
    fn flash_profiles_agree_with_unit_models() {
        // DCAS2 bootloader identities with no stock unit model entry; keep any
        // addition here deliberate.
        const UNMODELLED_LIVE: &[(u8, u8)] = &[(0x0a, 0), (0x0c, 0), (0x15, 0)];
        for profile in MOTOR_FLASH_PROFILES {
            let live = (profile.live_series, profile.live_unit);
            let names = crate::unit_models::candidates(live.0, live.1, None, None)
                .into_iter()
                .filter(|name| name.starts_with("DU-"))
                .collect::<Vec<_>>();
            assert_eq!(
                names.is_empty(),
                UNMODELLED_LIVE.contains(&live),
                "live {:02x}/{:02x} unit models {names:?}",
                live.0,
                live.1
            );
            assert!(
                names.is_empty()
                    || names
                        .iter()
                        .any(|name| name.starts_with(profile.family_name)),
                "{} for live {:02x}/{:02x} is not among unit models {names:?}",
                profile.family_name,
                profile.live_series,
                profile.live_unit
            );
            for other in MOTOR_FLASH_PROFILES {
                if other.live_series == profile.live_series && other.live_unit == profile.live_unit
                {
                    assert_eq!(
                        other.family_name, profile.family_name,
                        "live {:02x}/{:02x} maps to two families",
                        profile.live_series, profile.live_unit
                    );
                }
            }
        }
    }

    #[test]
    fn companion_pair_and_version_checks() {
        let d = Firmware::from_bytes(&image(FirmwareKind::DcasX)).unwrap();
        let m = Firmware::from_bytes(&image(FirmwareKind::Renesas)).unwrap();
        assert!(d
            .validate_companion_version_for_pair(&m, Version([4, 1, 11, 0]))
            .is_ok());
        assert!(d
            .validate_companion_version_for_pair(&m, Version([4, 0, 0, 0]))
            .is_err());
    }

    #[test]
    fn audit_all_motor_firmware_pairs() {
        let Some(dir) = std::env::var_os("CRANKLINK_AUDIT_FIRMWARE_FIXTURES") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let cases = [
            (
                "DUEP800-D.4.2.0.dat",
                "DUEP800-M.4.1.11.dat",
                FirmwareKind::DcasX,
                0x23u8,
                1u8,
                Version([5, 0, 6, 1]),
            ),
            (
                "DUEP600-D.4.4.1.dat",
                "DUEP600-M.4.4.0.dat",
                FirmwareKind::DcasX,
                0x35, // EP801 alias for 0x30
                1,
                Version([5, 0, 6, 1]),
            ),
            (
                "DUEP500-D.4.1.0.dat",
                "DUEP500-M.4.1.0.dat",
                FirmwareKind::DcasX,
                0x37,
                1,
                Version([5, 0, 6, 1]),
            ),
            (
                "DUE5100-D.4.1.0.dat",
                "DUE5100-M.4.0.0.dat",
                FirmwareKind::DcasX,
                0x38,
                1,
                Version([5, 0, 6, 1]),
            ),
            (
                "DUE6100-D.4.8.0.dat",
                "DUE6100-M.4.7.6.dat",
                FirmwareKind::DcasX,
                0x21, // E7000 alias for 0x20
                1,
                Version([5, 0, 6, 1]),
            ),
            (
                "DUE5000-D.4.5.0.dat",
                "DUE5000-M.4.4.8.dat",
                FirmwareKind::DcasX,
                0x22,
                1,
                Version([5, 0, 6, 1]),
            ),
            (
                "DUE8000-D.4.10.0.dat",
                "DUE8000-M.4.9.4.dat",
                FirmwareKind::Dcas2i,
                0x12,
                1,
                Version([4, 1, 0, 0]),
            ),
            (
                "2DUE8000-D.4.10.0.dat",
                "2DUE8000-M.4.9.4.dat",
                FirmwareKind::Dcas2i,
                0x12,
                2,
                Version([4, 1, 0, 0]),
            ),
        ];
        for (dname, mname, expected_kind, live_series, board, bl) in cases {
            let d = Firmware::load(dir.join(dname)).unwrap();
            let m = Firmware::load(dir.join(mname)).unwrap();
            assert_eq!(d.metadata.kind, expected_kind, "{dname}");
            assert_eq!(m.metadata.kind, FirmwareKind::Renesas, "{mname}");
            assert_eq!(d.metadata.board, Some(board), "{dname}");
            d.validate_companion(&m).unwrap();
            m.validate_companion(&d).unwrap();
            d.validate_companion_version_for_pair(&m, m.metadata.version)
                .unwrap();
            if live_series == 0x23 {
                d.validate_target(live_series, 0, board, bl).unwrap();
            } else {
                assert!(d.validate_target(live_series, 0, board, bl).is_err());
                d.validate_target_with_policy(live_series, 0, board, bl, true)
                    .unwrap();
            }
        }
    }
}
