//! Component names derived from E-TUBE stock identity and optional discriminators.
//!
//! Discovery, component scope and read commands: `docs/etube-protocol.md`.
//! Naming a component does not imply any settings/authentication support.

#[derive(Debug, Clone, Copy)]
pub struct UnitModel {
    pub series: u8,
    pub unit: u8,
    pub name: &'static str,
    pub part: Option<u8>,
    pub model_params: Option<&'static [u8]>,
}

/// Known model candidates. Stock identity alone can be ambiguous; never pick the
/// first candidate when multiple names share a series/unit pair.
pub const UNIT_MODELS: &[UnitModel] = &[
    UnitModel {
        series: 0x01,
        unit: 0x09,
        name: "SM-PCE1",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x00,
        name: "SM-BMR1",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x01,
        name: "ST-6770-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x01,
        name: "ST-6770-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x01,
        name: "SW-R600",
        part: Some(3),
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x01,
        name: "SW-R671-L",
        part: Some(6),
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x01,
        name: "SW-R671-R",
        part: Some(5),
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x02,
        name: "SM-EW67-A-E",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x03,
        name: "FD-6770",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x02,
        unit: 0x04,
        name: "RD-6770",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x03,
        unit: 0x05,
        name: "820-00-878",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x03,
        unit: 0x06,
        name: "820-00-876",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x03,
        unit: 0x07,
        name: "820-00-877",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x00,
        name: "SM-BMR2",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x01,
        name: "ST-9070-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x01,
        name: "ST-9070-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x01,
        name: "ST-9071-L",
        part: Some(10),
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x01,
        name: "ST-9071-R",
        part: Some(9),
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x01,
        name: "SW-9071-L",
        part: Some(8),
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x01,
        name: "SW-9071-R",
        part: Some(7),
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x03,
        name: "FD-9070",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x04,
        name: "RD-9070",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x08,
        name: "SM-EWW01",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x04,
        unit: 0x09,
        name: "SM-BCR2",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x05,
        unit: 0x01,
        name: "ST-R785-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x05,
        unit: 0x01,
        name: "ST-R785-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x06,
        unit: 0x01,
        name: "ST-S705-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x06,
        unit: 0x01,
        name: "SW-S705",
        part: Some(3),
        model_params: None,
    },
    UnitModel {
        series: 0x06,
        unit: 0x04,
        name: "MU-S705",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x01,
        name: "ST-6870-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x01,
        name: "ST-6870-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x01,
        name: "ST-6871-L",
        part: Some(10),
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x01,
        name: "ST-6871-R",
        part: Some(9),
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x03,
        name: "FD-6870",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x04,
        name: "RD-6870-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x07,
        unit: 0x04,
        name: "RD-6870-SS",
        part: Some(0),
        model_params: None,
    },
    // Historical E6000-family aliases share 08/00 in this catalog. Observed
    // application identities also distinguish E6001-family 0C/00 and 0A/00,
    // and E6002/E6012 15/00, from E6000 08/00. The aliases below are retained
    // candidates, not an authoritative live-wire identity map for that family.
    UnitModel {
        series: 0x08,
        unit: 0x00,
        name: "DU-E6000",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x08,
        unit: 0x00,
        name: "DU-E6001",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x08,
        unit: 0x00,
        name: "DU-E6002",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x08,
        unit: 0x00,
        name: "DU-E6010",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x08,
        unit: 0x00,
        name: "DU-E6012",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x08,
        unit: 0x00,
        name: "DU-E6050",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x08,
        unit: 0x01,
        name: "SW-E6000",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x01,
        name: "SW-M9050-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x01,
        name: "SW-M9050-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x02,
        name: "SC-M9050",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x03,
        name: "FD-M9050",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x03,
        name: "FD-M9070",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x04,
        name: "RD-M9050-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x04,
        name: "RD-M9050-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x05,
        name: "820-09-006",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x06,
        name: "820-09-002",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x09,
        unit: 0x07,
        name: "820-09-010",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x0E,
        unit: 0x00,
        name: "SM-RD79A",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x0E,
        unit: 0x04,
        name: "RD-7970-A",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x0F,
        unit: 0x04,
        name: "RD-6770-A",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x10,
        unit: 0x00,
        name: "BM-DN100",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x10,
        unit: 0x01,
        name: "SW-M8050-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x10,
        unit: 0x01,
        name: "SW-M8050-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x10,
        unit: 0x03,
        name: "FD-M8070",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x10,
        unit: 0x04,
        name: "RD-M8050-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x10,
        unit: 0x08,
        name: "EW-WU111",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x00,
        name: "BT-DN110",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9150-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9150-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9160-L",
        part: Some(10),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9160-R",
        part: Some(9),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9170-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9170-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9180-L",
        part: Some(15),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "ST-R9180-R",
        part: Some(14),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "SW-R9150-L",
        part: Some(4),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "SW-R9150-R",
        part: Some(3),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "SW-R9160-L",
        part: Some(8),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x01,
        name: "SW-R9160-R",
        part: Some(7),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x03,
        name: "FD-R9150",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x04,
        name: "RD-R9150",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x11,
        unit: 0x08,
        name: "EW-WU101",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x12,
        unit: 0x00,
        name: "DU-E8000",
        part: None,
        model_params: Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x12,
        unit: 0x00,
        name: "DU-E8080",
        part: None,
        model_params: Some(&[0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x12,
        unit: 0x01,
        name: "SW-E8000-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x12,
        unit: 0x02,
        name: "SC-E8000",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x13,
        unit: 0x02,
        name: "SC-M9051",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x01,
        name: "ST-R8050-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x01,
        name: "ST-R8050-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x01,
        name: "ST-R8060-L",
        part: Some(10),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x01,
        name: "ST-R8060-R",
        part: Some(9),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x01,
        name: "ST-R8070-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x01,
        name: "ST-R8070-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x03,
        name: "FD-R8050",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x04,
        name: "RD-R8050-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x04,
        name: "RD-R8050-SS",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x14,
        unit: 0x04,
        name: "RD-RX805-GS",
        part: Some(3),
        model_params: None,
    },
    UnitModel {
        series: 0x15,
        unit: 0x01,
        name: "SW-E6010-L",
        part: Some(11),
        model_params: None,
    },
    UnitModel {
        series: 0x15,
        unit: 0x01,
        name: "SW-E6010-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x16,
        unit: 0x01,
        name: "ST-RX815-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x16,
        unit: 0x01,
        name: "ST-RX815-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x16,
        unit: 0x03,
        name: "FD-RX815",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x16,
        unit: 0x04,
        name: "RD-RX815",
        part: Some(4),
        model_params: None,
    },
    UnitModel {
        series: 0x16,
        unit: 0x04,
        name: "RD-RX817",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x20,
        unit: 0x00,
        name: "DU-E6100",
        part: None,
        model_params: Some(&[0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x20,
        unit: 0x00,
        name: "DU-E6100-CRG",
        part: None,
        model_params: Some(&[0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x20,
        unit: 0x00,
        name: "DU-E6110",
        part: None,
        model_params: Some(&[0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x20,
        unit: 0x00,
        name: "DU-E6180",
        part: None,
        model_params: Some(&[0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x20,
        unit: 0x00,
        name: "DU-E6180-TRK",
        part: None,
        model_params: Some(&[0x18, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x20,
        unit: 0x01,
        name: "EW-SW100",
        part: Some(16),
        model_params: None,
    },
    UnitModel {
        series: 0x20,
        unit: 0x02,
        name: "SC-E6100",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x20,
        unit: 0x04,
        name: "MU-UR500",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x21,
        unit: 0x00,
        name: "DU-E7000",
        part: None,
        model_params: Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x21,
        unit: 0x01,
        name: "SW-E7000-L",
        part: Some(11),
        model_params: None,
    },
    UnitModel {
        series: 0x21,
        unit: 0x01,
        name: "SW-E7000-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x21,
        unit: 0x02,
        name: "SC-E7000",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x21,
        unit: 0x04,
        name: "MU-UR510",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x22,
        unit: 0x00,
        name: "DU-E5000",
        part: None,
        model_params: Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x22,
        unit: 0x00,
        name: "DU-E5080",
        part: None,
        model_params: Some(&[0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x22,
        unit: 0x00,
        name: "DU-E5080-H",
        part: None,
        model_params: Some(&[0x08, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x22,
        unit: 0x00,
        name: "DU-E5080-TRK",
        part: None,
        model_params: Some(&[0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x22,
        unit: 0x02,
        name: "EW-EN100",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x23,
        unit: 0x00,
        name: "DU-EP800",
        part: None,
        model_params: Some(&[0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x23,
        unit: 0x00,
        name: "DU-EP800-CRG",
        part: None,
        model_params: Some(&[0x30, 0x30, 0x2D, 0x43, 0x52, 0x47, 0x00]),
    },
    UnitModel {
        series: 0x23,
        unit: 0x01,
        name: "EW-SW300",
        part: Some(16),
        model_params: None,
    },
    UnitModel {
        series: 0x23,
        unit: 0x01,
        name: "SW-EM800-L",
        part: Some(11),
        model_params: None,
    },
    UnitModel {
        series: 0x23,
        unit: 0x02,
        name: "SC-EM800",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x25,
        unit: 0x00,
        name: "BT-DN300",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x25,
        unit: 0x01,
        name: "ST-R9270-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x25,
        unit: 0x01,
        name: "ST-R9270-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x25,
        unit: 0x03,
        name: "FD-R9250",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x25,
        unit: 0x04,
        name: "RD-R9250",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x27,
        unit: 0x01,
        name: "ST-R8170-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x27,
        unit: 0x01,
        name: "ST-R8170-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x27,
        unit: 0x03,
        name: "FD-R8150",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x27,
        unit: 0x04,
        name: "RD-R8150",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x28,
        unit: 0x01,
        name: "ST-R9250-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x28,
        unit: 0x01,
        name: "ST-R9250-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x29,
        unit: 0x01,
        name: "ST-R8150-L",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x29,
        unit: 0x01,
        name: "ST-R8150-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x2A,
        unit: 0x0D,
        name: "EW-EX300",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x2B,
        unit: 0x00,
        name: "FH-U6060",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x2B,
        unit: 0x04,
        name: "RD-U6050",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x2C,
        unit: 0x04,
        name: "RD-U6070",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x2D,
        unit: 0x01,
        name: "ST-R7170-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x2D,
        unit: 0x01,
        name: "ST-R7170-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x2D,
        unit: 0x03,
        name: "FD-R7150",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x2D,
        unit: 0x04,
        name: "RD-R7150",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x2E,
        unit: 0x01,
        name: "SW-M8150-R/IR",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x2E,
        unit: 0x04,
        name: "RD-M8150-11",
        part: Some(6),
        model_params: None,
    },
    UnitModel {
        series: 0x2E,
        unit: 0x04,
        name: "RD-M8150-12",
        part: Some(7),
        model_params: None,
    },
    UnitModel {
        series: 0x2F,
        unit: 0x00,
        name: "EW-EX310",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x30,
        unit: 0x00,
        name: "DU-EP600",
        part: None,
        model_params: Some(&[0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x30,
        unit: 0x00,
        name: "DU-EP600-CRG",
        part: None,
        model_params: Some(&[0x30, 0x30, 0x2D, 0x43, 0x52, 0x47, 0x00]),
    },
    UnitModel {
        series: 0x30,
        unit: 0x01,
        name: "SW-EN600-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x30,
        unit: 0x02,
        name: "SC-EN500",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x31,
        unit: 0x02,
        name: "SC-EN600",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x32,
        unit: 0x02,
        name: "SC-EN610",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x33,
        unit: 0x02,
        name: "SW-EN600-L",
        part: Some(11),
        model_params: None,
    },
    UnitModel {
        series: 0x34,
        unit: 0x02,
        name: "EW-SW310",
        part: Some(16),
        model_params: None,
    },
    UnitModel {
        series: 0x35,
        unit: 0x00,
        name: "DU-EP801",
        part: None,
        model_params: Some(&[0x30, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x35,
        unit: 0x00,
        name: "DU-EP801-CRG",
        part: None,
        model_params: Some(&[0x30, 0x31, 0x2D, 0x43, 0x52, 0x47, 0x00]),
    },
    UnitModel {
        series: 0x37,
        unit: 0x00,
        name: "DU-EP500",
        part: None,
        model_params: Some(&[0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x38,
        unit: 0x00,
        name: "DU-E5100",
        part: None,
        model_params: Some(&[0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    },
    UnitModel {
        series: 0x39,
        unit: 0x01,
        name: "ST-RX825-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x39,
        unit: 0x01,
        name: "ST-RX825-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x39,
        unit: 0x03,
        name: "FD-RX825",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x39,
        unit: 0x04,
        name: "RD-RX825",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x3B,
        unit: 0x04,
        name: "RD-U8050-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x3B,
        unit: 0x04,
        name: "RD-U8050-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x3C,
        unit: 0x01,
        name: "SW-EN605-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x3C,
        unit: 0x04,
        name: "MU-UR520",
        part: None,
        model_params: None,
    },
    UnitModel {
        series: 0x3D,
        unit: 0x01,
        name: "SW-M9250-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x3D,
        unit: 0x04,
        name: "RD-M9250-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x3D,
        unit: 0x04,
        name: "RD-M9250-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x3E,
        unit: 0x04,
        name: "RD-M9260-11L",
        part: Some(6),
        model_params: None,
    },
    UnitModel {
        series: 0x3E,
        unit: 0x04,
        name: "RD-M9260-12",
        part: Some(7),
        model_params: None,
    },
    UnitModel {
        series: 0x3F,
        unit: 0x01,
        name: "SW-M8250-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x3F,
        unit: 0x04,
        name: "RD-M8250-GS",
        part: Some(1),
        model_params: None,
    },
    UnitModel {
        series: 0x3F,
        unit: 0x04,
        name: "RD-M8250-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x41,
        unit: 0x01,
        name: "SW-M6250-R",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x41,
        unit: 0x04,
        name: "RD-M6250-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x42,
        unit: 0x04,
        name: "RD-M6260-12",
        part: Some(7),
        model_params: None,
    },
    UnitModel {
        series: 0x43,
        unit: 0x04,
        name: "RD-RX827-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x44,
        unit: 0x01,
        name: "SW-UT600-LR",
        part: Some(0),
        model_params: None,
    },
    UnitModel {
        series: 0x45,
        unit: 0x01,
        name: "ST-RX715-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x45,
        unit: 0x04,
        name: "RD-RX717-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x46,
        unit: 0x04,
        name: "RD-RX828-SGS",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x47,
        unit: 0x04,
        name: "RD-X8050",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x48,
        unit: 0x04,
        name: "RD-X7050",
        part: Some(2),
        model_params: None,
    },
    UnitModel {
        series: 0x4A,
        unit: 0x01,
        name: "ST-R9171-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x4A,
        unit: 0x01,
        name: "ST-R9171-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x4B,
        unit: 0x01,
        name: "ST-R8071-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x4B,
        unit: 0x01,
        name: "ST-R8071-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x5C,
        unit: 0x01,
        name: "ST-R9271-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x5C,
        unit: 0x01,
        name: "ST-R9271-R",
        part: Some(12),
        model_params: None,
    },
    UnitModel {
        series: 0x5D,
        unit: 0x01,
        name: "ST-R8171-L",
        part: Some(13),
        model_params: None,
    },
    UnitModel {
        series: 0x5D,
        unit: 0x01,
        name: "ST-R8171-R",
        part: Some(12),
        model_params: None,
    },
];

/// Resolve only with identity data supplied by the current discovery session.
/// `part` is ST_PART_GET/PCA_PART_GET; `model_params` is PU_MD_MODEL_GET.
/// Compare model parameters as bytes: older product attributes need not be
/// text. Firmware distribution names are not discriminators; EP600 and EP801,
/// for example, share a distribution family but have distinct stock identities.
pub fn candidates(
    series: u8,
    unit: u8,
    part: Option<u8>,
    model_params: Option<&[u8]>,
) -> Vec<&'static str> {
    UNIT_MODELS
        .iter()
        .filter(|entry| {
            entry.series == series
                && entry.unit == unit
                && match (part, entry.part) {
                    (Some(actual), Some(expected)) => actual == expected,
                    _ => true,
                }
                && match (model_params, entry.model_params) {
                    (Some(actual), Some(expected)) => actual == expected,
                    _ => true,
                }
        })
        .map(|entry| entry.name)
        .collect()
}

/// An exact model requires a unique candidate after applying available identity data.
pub fn identify(
    series: u8,
    unit: u8,
    part: Option<u8>,
    model_params: Option<&[u8]>,
) -> Option<&'static str> {
    let names = candidates(series, unit, part, model_params);
    if names.len() == 1 {
        Some(names[0])
    } else {
        None
    }
}

/// Extra identity data read without entering a motor session.
#[derive(Debug, serde::Serialize)]
pub struct ResolvedModel {
    pub model: Option<&'static str>,
    pub model_candidates: Vec<&'static str>,
    pub part: Option<u8>,
    pub model_params: Option<Vec<u8>>,
}

/// Resolve a stock pair using only vendor identity GET commands. The caller must
/// supply the stock pair and slot from the same discovery session. Failures are
/// returned, never retried or turned into a guessed component identity.
pub fn read_identification(
    commands: &mut (impl crate::device::UnitCommands + ?Sized),
    slot: u8,
    series: u8,
    unit: u8,
) -> anyhow::Result<ResolvedModel> {
    let entries: Vec<_> = UNIT_MODELS
        .iter()
        .filter(|entry| entry.series == series && entry.unit == unit)
        .collect();
    let mut part = None;
    let mut model_params = None;
    if entries.iter().any(|entry| entry.part.is_some()) {
        let (group, command, response) = match unit {
            1 | 2 => (4, 0x0C, 0x0E), // ST_PART_GET: switches/levers, including SD300 unit 2
            3 | 4 => (0x32, 0x9C, 0x9E), // PCA_PART_GET: front/rear derailleur type
            _ => anyhow::bail!(
                "no verified part identity command for series {series:02X} unit {unit:02X}"
            ),
        };
        let reply = commands.command(slot, group, command, &[0], response)?;
        anyhow::ensure!(
            !reply.is_empty(),
            "truncated component part identity at slot {slot}"
        );
        part = Some(reply[0]);
    }
    if entries.iter().any(|entry| entry.model_params.is_some()) {
        let reply = commands.command(slot, 0x16, 0x7C, &[0], 0x7E)?;
        anyhow::ensure!(
            reply.len() >= 7,
            "truncated component model discriminator at slot {slot}"
        );
        model_params = Some(reply[..7].to_vec());
    }
    let model_candidates = candidates(series, unit, part, model_params.as_deref());
    let model = if model_candidates.len() == 1 {
        Some(model_candidates[0])
    } else {
        None
    };
    Ok(ResolvedModel {
        model,
        model_candidates,
        part,
        model_params,
    })
}

/// Case-insensitive model matching permits the conventional optional hyphens.
/// It does not allow substrings, so EP800 cannot select EP800-CRG by accident.
pub fn name_matches(selector: &str, model: &str) -> bool {
    fn normalize(value: &str) -> String {
        value
            .chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .flat_map(char::to_uppercase)
            .collect()
    }
    let selector = normalize(selector);
    !selector.is_empty() && selector == normalize(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_catalog_resolves_known_stock_identities() {
        assert_eq!(identify(0x23, 2, None, None), Some("SC-EM800"));
        assert_eq!(identify(0x10, 4, None, None), Some("RD-M8050-GS"));
        assert_eq!(identify(0x23, 1, Some(11), None), Some("SW-EM800-L"));
        assert_eq!(identify(0x23, 1, Some(16), None), Some("EW-SW300"));
        assert_eq!(identify(0x21, 1, Some(11), None), Some("SW-E7000-L"));
        assert_eq!(identify(0x21, 1, Some(2), None), Some("SW-E7000-R"));
    }

    #[test]
    fn stock_pairs_do_not_guess_between_shared_model_names() {
        assert_eq!(identify(0x23, 0, None, None), None);
        assert_eq!(identify(0x23, 1, None, None), None);
        assert_eq!(identify(0x21, 1, None, None), None);
        assert_eq!(
            candidates(0x23, 0, None, None),
            ["DU-EP800", "DU-EP800-CRG"]
        );
        assert!(candidates(0x23, 1, Some(255), None).is_empty());
        assert!(candidates(0xFE, 0, None, None).is_empty());
    }

    #[test]
    fn motor_discrimination_requires_complete_matching_bytes() {
        assert_eq!(
            identify(0x23, 0, None, Some(b"00-CRG\0")),
            Some("DU-EP800-CRG")
        );
        assert_eq!(
            identify(0x23, 0, None, Some(b"00\0\0\0\0\0")),
            Some("DU-EP800")
        );
        assert_eq!(identify(0x23, 0, None, Some(b"00")), None);
        assert_eq!(identify(0x23, 0, None, Some(b"00-XYZ\0")), None);
    }

    #[test]
    fn selector_does_not_collapse_distinct_models() {
        assert!(name_matches("scem800", "SC-EM800"));
        assert!(name_matches("DU-EP800-CRG", "DU-EP800-CRG"));
        assert!(!name_matches("DU-EP800", "DU-EP800-CRG"));
        assert!(!name_matches("", "SC-EM800"));
    }

    #[test]
    fn identity_reads_are_limited_to_required_gets() {
        struct Mock {
            calls: Vec<(u8, u8, u8, Vec<u8>, u8)>,
            replies: std::collections::VecDeque<Vec<u8>>,
        }
        impl crate::device::UnitCommands for Mock {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                response: u8,
            ) -> anyhow::Result<Vec<u8>> {
                self.calls
                    .push((slot, group, command, params.to_vec(), response));
                self.replies
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("unexpected command"))
            }
        }
        let mut mock = Mock {
            calls: vec![],
            replies: [vec![11, 255], b"00-CRG\0".to_vec()].into(),
        };
        let switch = read_identification(&mut mock, 4, 0x23, 1).unwrap();
        assert_eq!(switch.model, Some("SW-EM800-L"));
        let motor = read_identification(&mut mock, 11, 0x23, 0).unwrap();
        assert_eq!(motor.model, Some("DU-EP800-CRG"));
        assert_eq!(
            mock.calls,
            [(4, 4, 0x0C, vec![0], 0x0E), (11, 0x16, 0x7C, vec![0], 0x7E)]
        );
        assert_eq!(
            read_identification(&mut mock, 19, 0x23, 2).unwrap().model,
            Some("SC-EM800")
        );
        assert!(read_identification(&mut mock, 27, 0xFE, 1)
            .unwrap()
            .model
            .is_none());
        assert_eq!(mock.calls.len(), 2);
        mock.replies.push_back(vec![]);
        assert!(read_identification(&mut mock, 4, 0x23, 1).is_err());
        assert_eq!(mock.calls.len(), 3); // no retry after malformed response
    }

    #[test]
    fn catalog_does_not_duplicate_models_for_a_stock_pair() {
        for (index, entry) in UNIT_MODELS.iter().enumerate() {
            assert!(!UNIT_MODELS[..index]
                .iter()
                .any(|previous| previous.series == entry.series
                    && previous.unit == entry.unit
                    && previous.name == entry.name));
        }
    }
}
