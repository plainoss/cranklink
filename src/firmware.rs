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
        ensure!(major <= 9 && minor <= 9, "invalid packed BCD version");
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
    Renesas,
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

    /// Write policy for the documented DU-EP800 family. Other families require
    /// an independently verified model mapping and writer, even if headers parse.
    pub fn validate_target(
        &self,
        series: u8,
        unit: u8,
        board: u8,
        bootloader: Version,
    ) -> Result<()> {
        let m = &self.metadata;
        ensure!(
            m.series == 0x23 && m.unit == 0,
            "only the verified DU-EP800 firmware family (23/00) is supported for writing"
        );
        ensure!(
            m.series == series && m.unit == unit,
            "firmware identity {:02X}/{:02X} does not match target {series:02X}/{unit:02X}",
            m.series,
            m.unit
        );
        if m.kind == FirmwareKind::DcasX {
            ensure!(
                m.board == Some(board),
                "firmware board {:?} does not match target board {board}",
                m.board
            );
            ensure!(
                board == 1,
                "only the verified DCASX board 1 is supported for writing"
            );
            ensure!(
                bootloader >= Version([5, 0, 0, 0]),
                "DCASX requires bootloader >= 5.0.0.0, found {bootloader}"
            );
            ensure!(
                !self.data.is_empty() && self.data.len().div_ceil(2048) <= 256,
                "DCASX image exceeds the one-byte sector erase-code range"
            );
        }
        Ok(())
    }

    /// Check a pair before programming either CPU. The D image advertises its
    /// required RX companion version; the M header does not require D >= M.
    pub fn validate_companion(&self, companion: &Self) -> Result<()> {
        ensure!(
            self.metadata.kind != companion.metadata.kind,
            "a firmware pair must contain one D and one M image"
        );
        ensure!(
            self.metadata.series == companion.metadata.series
                && self.metadata.unit == companion.metadata.unit,
            "companion firmware series/unit mismatch"
        );
        let (d, m) = if self.metadata.kind == FirmwareKind::DcasX {
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
            self.metadata.kind == FirmwareKind::DcasX,
            "companion version check requires a D image"
        );
        ensure!(
            actual == self.metadata.compatibility_version,
            "D firmware requires RX {}, found {actual}",
            self.metadata.compatibility_version
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
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;
    fn image(kind: FirmwareKind) -> Vec<u8> {
        let mut b = vec![0xff; 128];
        match kind {
            FirmwareKind::DcasX => {
                b[16..19].copy_from_slice(&[0x42, 0, 0]);
                b[24..28].copy_from_slice(&128u32.to_le_bytes());
                b[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
                b[47..50].copy_from_slice(&[0x41, 11, 0]);
            }
            FirmwareKind::Renesas => {
                b[..4].copy_from_slice(&128u32.to_le_bytes());
                b[8..12].copy_from_slice(&[0x41, 11, 0, 1]);
                b[14..19].copy_from_slice(&[0x23, 0, 0x41, 11, 0]);
            }
        }
        b[126..].copy_from_slice(b"et");
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
}
