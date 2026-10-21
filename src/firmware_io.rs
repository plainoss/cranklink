//! Logical firmware messages shared by wired and BLE motor transports.
//! Byte literals in wire layouts are hexadecimal; byte counts are decimal.
//!
//! DCAS programming distinguishes four receipts: adapter buffer acceptance
//! (`82`, data packet ID), transfer acceptance (`83`, transfer packet ID),
//! transfer completion (`C0`, data packet ID), and the motor's checksum receipt
//! (`31`, block index on DCAS2i/X). None substitutes for another or for finish.
//! Completion/checksum events can arrive before the current wait completes;
//! implementations must retain unmatched events for subsequent waits.
use crate::{
    device::{BootIdentity, FlashReceipt},
    platform::sleep,
    protocol::Frame,
};
use anyhow::{ensure, Context, Result};
use std::{borrow::Cow, time::Duration};

pub(crate) trait FirmwareIo {
    fn pace(&mut self, duration: Duration) {
        sleep(duration);
    }
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()>;
    fn wait(&mut self, timeout: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame>;
    fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool);
    fn discard_unindexed(&mut self) {
        self.discard_matching(crate::device::unindexed_dcas_ack);
    }
    fn exchange(
        &mut self,
        control: u8,
        payload: &[u8],
        timeout: Duration,
        check: impl Fn(&Frame) -> bool,
    ) -> Result<Frame> {
        self.discard_matching(&check);
        self.send(control, payload)?;
        self.wait(timeout, check)
    }
}
pub(crate) fn boot_query(
    link: &mut impl FirmwareIo,
    payload: &[u8],
    response: u8,
    min: usize,
    timeout: u64,
) -> Result<Vec<u8>> {
    let f = link.exchange(0x88, payload, Duration::from_millis(timeout), |f| {
        f.control & 0x88 == 0x88
            && f.payload.len() >= min
            && matches!(f.payload.first(), Some(&b) if b == response || b == 0x32)
    })?;
    ensure!(
        f.payload[0] == response,
        "motor boot command {:02X} rejected: {}",
        payload[0],
        hex::encode(&f.payload)
    );
    Ok(f.payload)
}
/// Read D boot identity after entry: `2E -> 3A` supplies packed version bytes,
/// `2F -> 3B` the last version byte and board, `30 -> 3C` series/unit, and
/// `29/2A -> 35/36` the two three-byte serial pieces. Each request has three
/// trailing zero bytes. These are boot payload opcodes, not normal group/opcodes;
/// `88` replies have no component route, so the caller must serialize ownership.
pub(crate) fn boot_identity(link: &mut impl FirmwareIo, slot: u8) -> Result<BootIdentity> {
    let lo = boot_query(link, &[0x2e, 0, 0, 0], 0x3a, 3, 3000)?;
    let hi = boot_query(link, &[0x2f, 0, 0, 0], 0x3b, 3, 3000)?;
    let unit = boot_query(link, &[0x30, 0, 0, 0], 0x3c, 3, 3000)?;
    let a = boot_query(link, &[0x29, 0, 0, 0], 0x35, 4, 3000)?;
    let b = boot_query(link, &[0x2a, 0, 0, 0], 0x36, 4, 3000)?;
    Ok(BootIdentity {
        slot,
        version: [lo[1] >> 4, lo[1] & 15, lo[2], hi[1]],
        board: hi[2],
        series: unit[1],
        unit: unit[2],
        serial: [a[1], a[2], a[3], b[1], b[2], b[3]],
    })
}
pub(crate) fn boot_reset(link: &mut impl FirmwareIo) -> Result<()> {
    boot_query(link, &[0x28, 0, 0, 0], 0x31, 1, 3000)?;
    Ok(())
}
pub(crate) fn flash_dcas(
    link: &mut impl FirmwareIo,
    image: &[u8],
    identity: &BootIdentity,
    progress: impl FnMut(usize, usize),
) -> Result<FlashReceipt> {
    flash_dcas_with_user_info(link, image, identity, None, progress)
}

/// Program DCAS2/2i/X using 64-byte, `FF`-padded blocks. The `0B` buffer payload
/// is `[packet, 02, 00, data64, block_sum, index_lo, index_hi]`, followed by a
/// separate `[next_packet, 03, data_packet]` transfer request. Packet IDs wrap
/// modulo 256. DCAS2i/X indexes are one-based LE16; older DCAS sends index zero
/// and selects a second page at 64 KiB. The block checksum includes padding;
/// the final image checksum includes only real bytes (after user-info changes).
///
/// Before data, `21 00 erase_code 00` erases/starts programming, `0A page 00 00`
/// selects a page and `24 00 40 00` sets logical address 0x4000. DCASX erase code
/// is `(length - 1) / 2048`; older layouts use size classes 0 (<=32 KiB),
/// 1 (<=64 KiB), or 2 (>64 KiB). Newer boot versions first receive target
/// series/unit and serial via commands 06 and 07..09. Do not interchange the
/// erase rules or replay erase after an uncertain outcome. Finish uses
/// `27 image_sum 00 00`, reset `28 00 00 00`; all expect `31`, but neither
/// receipt establishes that the normal application returned successfully.
pub(crate) fn flash_dcas_with_user_info(
    link: &mut impl FirmwareIo,
    image: &[u8],
    identity: &BootIdentity,
    user_info: Option<[u8; 2]>,
    mut progress: impl FnMut(usize, usize),
) -> Result<FlashReceipt> {
    ensure!(
        image.len() >= 50 && image.len() <= 256 * 2048,
        "invalid DCASX image length"
    );
    let (fw_series, fw_unit, dcas_ver, fw_board) = if image.len() >= 0xd6
        && image[..4] == [0xff; 4]
        && image[7] == 0
        && image[11] == 0
        && image[15] == 0
        && image[0xc0] == 0x05
        && matches!(image[0xcc], 1..=3)
    {
        ensure!(image.len() <= 2 * 65536, "invalid DCAS2/2i image length");
        (image[0xca], image[0xcb], image[0xcc], image[0xcd])
    } else {
        let (fw_series, fw_unit, dcas_ver, fw_board) = (image[40], image[41], image[42], image[43]);
        ensure!(dcas_ver == 4, "image/motor compatibility mismatch");
        (fw_series, fw_unit, dcas_ver, fw_board)
    };

    let live = boot_identity(link, identity.slot)?;
    ensure!(
        live.slot == identity.slot
            && live.version == identity.version
            && live.serial == identity.serial
            && live.series == identity.series
            && live.unit == identity.unit
            && live.board == identity.board,
        "live motor changed"
    );

    let series_unit_match = (fw_series == live.series && fw_unit == live.unit)
        || crate::firmware::find_motor_profile(live.series, live.unit, fw_series, fw_unit)
            .is_some();
    let live_version = crate::firmware::Version(live.version);
    let bootloader_board_match = match dcas_ver {
        4 => live.version[0] >= 5 && fw_board == live.board,
        3 => live.version[0] == 4 && fw_board == live.board,
        1 | 2 => {
            live_version >= crate::firmware::Version([3, 2, 0, 0])
                && live.version[0] < 4
                && (live_version < crate::firmware::Version([3, 2, 4, 0]) || fw_board == live.board)
        }
        _ => false,
    };
    ensure!(
        series_unit_match && bootloader_board_match,
        "image/motor compatibility mismatch"
    );

    let payload: Cow<'_, [u8]> = if dcas_ver <= 2 {
        let mut buf = image.to_vec();
        let ui = user_info.unwrap_or([0, 0]);
        buf[0xc8] = ui[1];
        buf[0xc9] = ui[0];
        Cow::Owned(buf)
    } else {
        Cow::Borrowed(image)
    };

    if crate::firmware::Version(live.version) >= crate::firmware::Version([3, 2, 3, 0]) {
        boot_query(link, &[6, live.series, live.unit, 0], 0x31, 1, 3000)?;
        for i in 0..3 {
            boot_query(
                link,
                &[7 + i as u8, live.serial[i * 2], live.serial[i * 2 + 1], 0],
                0x31,
                1,
                3000,
            )?;
        }
    }

    let erase_code = if dcas_ver == 4 {
        ((payload.len() - 1) / 2048) as u8
    } else if payload.len() > 65536 {
        2
    } else if payload.len() > 32768 {
        1
    } else {
        0
    };
    boot_query(link, &[0x21, 0, erase_code, 0], 0x31, 1, 5000)
        .context("erase outcome uncertain; do not automatically restart")?;

    boot_query(link, &[0x0a, 0, 0, 0], 0x31, 1, 1200)?;
    boot_query(link, &[0x24, 0, 0x40, 0], 0x31, 1, 1200)?;

    let is_dcas2i = dcas_ver >= 3;
    let mut packet = 255u8;
    for (i, chunk) in payload.chunks(64).enumerate() {
        let index = (i + 1) as u16;
        if !is_dcas2i && i * 64 == 0x10000 {
            boot_query(link, &[0x0a, 1, 0, 0], 0x31, 1, 1200)?;
            boot_query(link, &[0x24, 0, 0x40, 0], 0x31, 1, 1200)?;
        }
        let mut block = [0xff; 64];
        block[..chunk.len()].copy_from_slice(chunk);
        let sum = block.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        packet = packet.wrapping_add(1);
        let packet_index = if is_dcas2i { index } else { 0 };
        let p = crate::device::dcas_buffer_packet(packet, packet_index, &block, sum);
        link.exchange(0x0b, &p, Duration::from_secs(3), |f| {
            f.control & 0x0b == 0x0b
                && f.payload.len() >= 4
                && f.payload[1] == 0x82
                && f.payload[2] == packet
                && f.payload[3] == 0
        })
        .with_context(|| format!("buffer block {index}; outcome uncertain"))?;
        let data_packet = packet;
        packet = packet.wrapping_add(1);
        let transfer_packet = packet;
        link.exchange(
            0x0b,
            &[packet, 3, data_packet],
            Duration::from_secs(25),
            |f| {
                f.control & 0x0b == 0x0b
                    && f.payload.len() >= 4
                    && f.payload[1] == 0x83
                    && f.payload[2] == transfer_packet
                    && f.payload[3] == 0
            },
        )
        .with_context(|| format!("transfer start block {index}; outcome uncertain"))?;
        link.wait(Duration::from_secs(25), |f| {
            f.control & 0x0b == 0x0b
                && f.payload.len() >= 3
                && f.payload[1] == 0xc0
                && f.payload[2] == data_packet
        })
        .with_context(|| format!("transfer completion block {index}; outcome uncertain"))?;
        if is_dcas2i {
            link.wait(Duration::from_secs(25), |f| {
                f.control & 0x88 == 0x88
                    && f.payload.len() >= 3
                    && f.payload[0] == 0x31
                    && u16::from_le_bytes([f.payload[1], f.payload[2]]) == index
            })
            .with_context(|| format!("motor checksum block {index}; outcome uncertain"))?;
            // Generic `31 00 00` receipts do not prove an indexed block. Drop
            // those only after the matching LE16 index has been acknowledged.
            link.discard_unindexed();
        } else {
            link.wait(Duration::from_secs(25), |f| {
                f.control & 0x88 == 0x88 && f.payload.first() == Some(&0x31)
            })
            .with_context(|| format!("motor checksum block {index}; outcome uncertain"))?;
        }
        progress((i * 64 + chunk.len()).min(payload.len()), payload.len());
    }
    let checksum = payload.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    boot_query(link, &[0x27, checksum, 0, 0], 0x31, 1, 3000).context("finish outcome uncertain")?;
    sleep(Duration::from_secs(1));
    boot_reset(link).context("reset outcome uncertain")?;
    Ok(FlashReceipt {
        bytes: payload.len(),
        checksum,
        finish_acknowledged: true,
        reset_acknowledged: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Replay {
        incoming: VecDeque<Frame>,
        sent: Vec<Frame>,
        image: Vec<u8>,
        identity: BootIdentity,
        blocks: usize,
        fail_completion: bool,
        wrong_serial: bool,
        dcas_ver: u8,
        expected_user_info: Option<[u8; 2]>,
    }
    fn frame(control: u8, payload: Vec<u8>) -> Frame {
        Frame { control, payload }
    }
    fn identity() -> BootIdentity {
        BootIdentity {
            slot: 13,
            version: [5, 0, 0, 0],
            board: 1,
            series: 0x23,
            unit: 0,
            serial: [1, 2, 3, 4, 5, 6],
        }
    }
    fn identity_dcas2i() -> BootIdentity {
        BootIdentity {
            slot: 13,
            version: [4, 1, 0, 0],
            board: 1,
            series: 0x12,
            unit: 0,
            serial: [1, 2, 3, 4, 5, 6],
        }
    }
    fn identity_dcas2() -> BootIdentity {
        BootIdentity {
            slot: 13,
            version: [3, 2, 4, 0],
            board: 1,
            series: 0x0c,
            unit: 0,
            serial: [1, 2, 3, 4, 5, 6],
        }
    }
    impl Replay {
        fn new(n: usize) -> Self {
            let mut image: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            image[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
            Self {
                incoming: VecDeque::new(),
                sent: Vec::new(),
                image,
                identity: identity(),
                blocks: 0,
                fail_completion: false,
                wrong_serial: false,
                dcas_ver: 4,
                expected_user_info: None,
            }
        }
        fn new_dcas2(n: usize, dcas_ver: u8, identity: BootIdentity) -> Self {
            let mut image: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            image[..4].copy_from_slice(&[0xff; 4]);
            image[7] = 0;
            image[11] = 0;
            image[15] = 0;
            image[0xc0] = 0x05;
            image[0xc8] = 0x11;
            image[0xc9] = 0x22;
            image[0xca] = identity.series;
            image[0xcb] = identity.unit;
            image[0xcc] = dcas_ver;
            image[0xcd] = identity.board;
            Self {
                incoming: VecDeque::new(),
                sent: Vec::new(),
                image,
                identity,
                blocks: 0,
                fail_completion: false,
                wrong_serial: false,
                dcas_ver,
                expected_user_info: None,
            }
        }
        fn push(&mut self, control: u8, payload: Vec<u8>) {
            self.incoming.push_back(frame(control, payload));
        }
    }
    impl FirmwareIo for Replay {
        fn send(&mut self, control: u8, p: &[u8]) -> Result<()> {
            self.sent.push(frame(control, p.to_vec()));
            if control == 0x88 {
                let r = match p[0] {
                    0x2e => vec![
                        0x3a,
                        (self.identity.version[0] << 4) | (self.identity.version[1] & 15),
                        self.identity.version[2],
                    ],
                    0x2f => vec![0x3b, self.identity.version[3], self.identity.board],
                    0x30 => vec![0x3c, self.identity.series, self.identity.unit],
                    0x29 => vec![
                        0x35,
                        if self.wrong_serial {
                            9
                        } else {
                            self.identity.serial[0]
                        },
                        self.identity.serial[1],
                        self.identity.serial[2],
                    ],
                    0x2a => vec![
                        0x36,
                        self.identity.serial[3],
                        self.identity.serial[4],
                        self.identity.serial[5],
                    ],
                    0x21 => {
                        let expected_erase = if self.dcas_ver == 4 {
                            ((self.image.len() - 1) / 2048) as u8
                        } else if self.image.len() > 65536 {
                            2
                        } else if self.image.len() > 32768 {
                            1
                        } else {
                            0
                        };
                        assert_eq!(p, [0x21, 0, expected_erase, 0]);
                        vec![0x31, 0, 0]
                    }
                    0x27 => {
                        let mut expected_payload = self.image.clone();
                        if self.dcas_ver <= 2 {
                            let ui = self.expected_user_info.unwrap_or([0, 0]);
                            expected_payload[0xc8] = ui[1];
                            expected_payload[0xc9] = ui[0];
                        }
                        let expected_sum =
                            expected_payload.iter().fold(0u8, |a, b| a.wrapping_add(*b));
                        assert_eq!(p[1], expected_sum);
                        assert_eq!(self.blocks, self.image.len().div_ceil(64));
                        vec![0x31, 0, 0]
                    }
                    _ => vec![0x31, 0, 0],
                };
                self.push(0x88, r);
            } else if p[1] == 2 {
                let index = self.blocks + 1;
                assert_eq!(p.len(), 70);
                assert_eq!(p[0], ((index - 1) * 2 % 256) as u8);
                let expected_idx = if self.dcas_ver >= 3 {
                    (index as u16).to_le_bytes()
                } else {
                    [0, 0]
                };
                assert_eq!(&p[68..70], &expected_idx);
                let offset = (index - 1) * 64;
                let n = (self.image.len() - offset).min(64);
                let mut expected_payload = self.image.clone();
                if self.dcas_ver <= 2 {
                    let ui = self.expected_user_info.unwrap_or([0, 0]);
                    expected_payload[0xc8] = ui[1];
                    expected_payload[0xc9] = ui[0];
                }
                assert_eq!(&p[3..3 + n], &expected_payload[offset..offset + n]);
                assert!(p[3 + n..67].iter().all(|b| *b == 255));
                assert_eq!(p[67], p[3..67].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
                self.push(0x0b, vec![0, 0x82, p[0], 0]);
            } else {
                assert_eq!(p[1], 3);
                let id = p[2];
                self.blocks += 1;
                let idx = (self.blocks as u16).to_le_bytes();
                if self.dcas_ver >= 3 {
                    // Motor and completion can arrive before transfer-start receipt.
                    self.push(0x88, vec![0x31, idx[0], idx[1]]);
                    self.push(0x88, vec![0x31, 0, 0]);
                } else {
                    self.push(0x88, vec![0x31, 0, 0]);
                }
                if !self.fail_completion {
                    self.push(0x0b, vec![0, 0xc0, id]);
                }
                self.push(0x0b, vec![0, 0x83, p[0], 0]);
            }
            Ok(())
        }
        fn wait(&mut self, _: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
            let i = self
                .incoming
                .iter()
                .position(check)
                .context("missing fixture receipt")?;
            Ok(self.incoming.remove(i).unwrap())
        }
        fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool) {
            self.incoming.retain(|f| !check(f));
        }
    }
    #[test]
    fn full_dcas_image_waits_for_all_receipts_across_packet_wrap() {
        let mut link = Replay::new(157996);
        let image = link.image.clone();
        let mut progress = 0;
        let r = flash_dcas(&mut link, &image, &identity(), |n, _| progress = n).unwrap();
        assert_eq!(progress, image.len());
        assert_eq!(r.bytes, image.len());
        assert!(r.finish_acknowledged && r.reset_acknowledged);
        assert!(link.incoming.is_empty());
        assert_eq!(
            link.sent
                .iter()
                .filter(|f| f.control == 0x88 && f.payload[0] == 0x21)
                .count(),
            1
        );
    }
    #[test]
    fn missing_completion_aborts_without_next_block_finish_or_retry() {
        let mut link = Replay::new(256);
        link.fail_completion = true;
        let image = link.image.clone();
        assert!(flash_dcas(&mut link, &image, &identity(), |_, _| {}).is_err());
        assert_eq!(link.blocks, 1);
        assert!(!link
            .sent
            .iter()
            .any(|f| f.control == 0x88 && matches!(f.payload[0], 0x27 | 0x28)));
        assert_eq!(
            link.sent
                .iter()
                .filter(|f| f.control == 0x88 && f.payload[0] == 0x21)
                .count(),
            1
        );
    }
    #[test]
    fn changed_boot_identity_blocks_all_programming_commands() {
        let mut link = Replay::new(256);
        link.wrong_serial = true;
        let image = link.image.clone();
        assert!(flash_dcas(&mut link, &image, &identity(), |_, _| {}).is_err());
        assert!(
            link.sent
                .iter()
                .all(|f| f.control == 0x88
                    && matches!(f.payload[0], 0x2e | 0x2f | 0x30 | 0x29 | 0x2a))
        );
    }
    #[test]
    fn changed_compatible_boot_version_blocks_all_programming_commands() {
        let mut link = Replay::new(256);
        // Both versions satisfy DCASX compatibility, but the exact expected
        // identity must remain unchanged through the final pre-erase check.
        link.identity.version[3] = 1;
        let image = link.image.clone();
        assert!(flash_dcas(&mut link, &image, &identity(), |_, _| {}).is_err());
        assert!(
            link.sent
                .iter()
                .all(|f| f.control == 0x88
                    && matches!(f.payload[0], 0x2e | 0x2f | 0x30 | 0x29 | 0x2a))
        );
        assert_eq!(link.blocks, 0);
    }
    #[test]
    fn dcas2i_flashing_uses_coarse_erase_and_indexed_packets() {
        let id = identity_dcas2i();
        let mut link = Replay::new_dcas2(70000, 3, id.clone());
        let image = link.image.clone();
        let mut progress = 0;
        let r = flash_dcas(&mut link, &image, &id, |n, _| progress = n).unwrap();
        assert_eq!(progress, image.len());
        assert_eq!(r.bytes, image.len());
        assert!(r.finish_acknowledged && r.reset_acknowledged);

        // Verify coarse erase code 2 for >64K image:
        assert_eq!(
            link.sent
                .iter()
                .filter(|f| f.control == 0x88 && f.payload == [0x21, 0, 2, 0])
                .count(),
            1
        );

        // Verify indexed packets (index > 0):
        let buffer_blocks: Vec<_> = link
            .sent
            .iter()
            .filter(|f| f.control == 0x0b && f.payload.len() == 70 && f.payload[1] == 2)
            .collect();
        assert_eq!(buffer_blocks.len(), 70000usize.div_ceil(64));
        for (i, b) in buffer_blocks.iter().enumerate() {
            let expected_idx = ((i + 1) as u16).to_le_bytes();
            assert_eq!(&b.payload[68..70], &expected_idx);
        }

        // Verify no page switch for DCAS2i even though >64 KiB:
        assert!(link
            .sent
            .iter()
            .all(|f| !(f.control == 0x88 && f.payload.starts_with(&[0x0a, 1]))));
    }
    #[test]
    fn dcas2_flashing_patches_user_info_switches_pages_and_uses_unindexed_packets() {
        let id = identity_dcas2();
        let mut link = Replay::new_dcas2(65600, 2, id.clone());
        let image = link.image.clone();
        let mut progress = 0;
        let r = flash_dcas(&mut link, &image, &id, |n, _| progress = n).unwrap();
        assert_eq!(progress, image.len());
        assert_eq!(r.bytes, image.len());
        assert!(r.finish_acknowledged && r.reset_acknowledged);

        // Verify erase code 2 for >64K image:
        assert_eq!(
            link.sent
                .iter()
                .filter(|f| f.control == 0x88 && f.payload == [0x21, 0, 2, 0])
                .count(),
            1
        );

        // Verify page switch to page 1 at 64 KiB boundary:
        assert_eq!(
            link.sent
                .iter()
                .filter(|f| f.control == 0x88 && f.payload == [0x0a, 1, 0, 0])
                .count(),
            1
        );
        assert_eq!(
            link.sent
                .iter()
                .filter(|f| f.control == 0x88 && f.payload == [0x24, 0, 0x40, 0])
                .count(),
            2 // initial at startup + page switch
        );

        // Verify unindexed 0x0B buffer packets: &p[68..70] == [0, 0] for all blocks
        let buffer_blocks: Vec<_> = link
            .sent
            .iter()
            .filter(|f| f.control == 0x0b && f.payload.len() == 70 && f.payload[1] == 2)
            .collect();
        assert_eq!(buffer_blocks.len(), 65600usize.div_ceil(64));
        for b in &buffer_blocks {
            assert_eq!(&b.payload[68..70], &[0, 0]);
        }

        // Verify SetUserInfo patching: in original image image[0xc8] == 0x11 and image[0xc9] == 0x22,
        // but sent buffer packet has 0 at those positions.
        // 0xc8 is index 200, which is in block 3 (0-indexed: block 3 covers 192..256, offset 200 is index 3 + 8 = 11 in payload)
        let block_containing_user_info = &buffer_blocks[3];
        assert_eq!(block_containing_user_info.payload[3 + (0xc8 - 192)], 0);
        assert_eq!(block_containing_user_info.payload[3 + (0xc9 - 192)], 0);
    }
    #[test]
    fn dcas2_flashing_with_custom_user_info() {
        let id = identity_dcas2();
        let mut link = Replay::new_dcas2(256, 1, id.clone());
        link.expected_user_info = Some([0x55, 0x66]);
        let image = link.image.clone();
        let r = flash_dcas_with_user_info(&mut link, &image, &id, Some([0x55, 0x66]), |_, _| {})
            .unwrap();
        assert_eq!(r.bytes, image.len());
        let buffer_blocks: Vec<_> = link
            .sent
            .iter()
            .filter(|f| f.control == 0x0b && f.payload.len() == 70 && f.payload[1] == 2)
            .collect();
        let block_user_info = &buffer_blocks[3];
        assert_eq!(block_user_info.payload[3 + (0xc8 - 192)], 0x66);
        assert_eq!(block_user_info.payload[3 + (0xc9 - 192)], 0x55);
    }
}
