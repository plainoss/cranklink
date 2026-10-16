//! Logical firmware messages shared by wired and BLE motor transports.
use crate::{
    device::{BootIdentity, FlashReceipt},
    platform::sleep,
    protocol::Frame,
};
use anyhow::{ensure, Context, Result};
use std::time::Duration;

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
    mut progress: impl FnMut(usize, usize),
) -> Result<FlashReceipt> {
    ensure!(
        image.len() >= 50 && image.len() <= 256 * 2048,
        "invalid DCASX image length"
    );
    let live = boot_identity(link, identity.slot)?;
    ensure!(
        live.slot == identity.slot
            && live.serial == identity.serial
            && live.series == identity.series
            && live.unit == identity.unit
            && live.board == identity.board,
        "live motor changed"
    );
    ensure!(
        live.version[0] >= 5
            && image[40] == live.series
            && image[41] == live.unit
            && image[42] == 4
            && image[43] == live.board,
        "image/motor compatibility mismatch"
    );
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
    boot_query(
        link,
        &[0x21, 0, ((image.len() - 1) / 2048) as u8, 0],
        0x31,
        1,
        5000,
    )
    .context("erase outcome uncertain; do not automatically restart")?;
    boot_query(link, &[0x0a, 0, 0, 0], 0x31, 1, 1200)?;
    boot_query(link, &[0x24, 0, 0x40, 0], 0x31, 1, 1200)?;
    let mut packet = 255u8;
    for (i, chunk) in image.chunks(64).enumerate() {
        let index = (i + 1) as u16;
        let mut block = [0xff; 64];
        block[..chunk.len()].copy_from_slice(chunk);
        let sum = block.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        packet = packet.wrapping_add(1);
        let p = crate::device::dcas_buffer_packet(packet, index, &block, sum);
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
        link.wait(Duration::from_secs(25), |f| {
            f.control & 0x88 == 0x88
                && f.payload.len() >= 3
                && f.payload[0] == 0x31
                && u16::from_le_bytes([f.payload[1], f.payload[2]]) == index
        })
        .with_context(|| format!("motor checksum block {index}; outcome uncertain"))?;
        // The motor additionally publishes an unindexed success ACK for
        // each block. Index zero must not satisfy a block checksum wait.
        // It is not an acknowledgement of any image block; retain its raw
        // trace while removing it from the pending correlation queue.
        link.discard_unindexed();
        progress((i * 64 + chunk.len()).min(image.len()), image.len());
    }
    let checksum = image.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    boot_query(link, &[0x27, checksum, 0, 0], 0x31, 1, 3000).context("finish outcome uncertain")?;
    sleep(Duration::from_secs(1));
    boot_reset(link).context("reset outcome uncertain")?;
    Ok(FlashReceipt {
        bytes: image.len(),
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
        blocks: usize,
        fail_completion: bool,
        wrong_serial: bool,
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
    impl Replay {
        fn new(n: usize) -> Self {
            let mut image: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            image[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
            Self {
                incoming: VecDeque::new(),
                sent: Vec::new(),
                image,
                blocks: 0,
                fail_completion: false,
                wrong_serial: false,
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
                    0x2e => vec![0x3a, 0x50, 0],
                    0x2f => vec![0x3b, 0, 1],
                    0x30 => vec![0x3c, 0x23, 0],
                    0x29 => vec![0x35, if self.wrong_serial { 9 } else { 1 }, 2, 3],
                    0x2a => vec![0x36, 4, 5, 6],
                    0x21 => {
                        assert_eq!(p, [0x21, 0, ((self.image.len() - 1) / 2048) as u8, 0]);
                        vec![0x31, 0, 0]
                    }
                    0x27 => {
                        assert_eq!(p[1], self.image.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
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
                assert_eq!(&p[68..70], &(index as u16).to_le_bytes());
                let offset = (index - 1) * 64;
                let n = (self.image.len() - offset).min(64);
                assert_eq!(&p[3..3 + n], &self.image[offset..offset + n]);
                assert!(p[3 + n..67].iter().all(|b| *b == 255));
                assert_eq!(p[67], p[3..67].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
                self.push(0x0b, vec![0, 0x82, p[0], 0]);
            } else {
                assert_eq!(p[1], 3);
                let id = p[2];
                self.blocks += 1;
                let idx = (self.blocks as u16).to_le_bytes();
                // Motor and completion can arrive before transfer-start receipt.
                self.push(0x88, vec![0x31, idx[0], idx[1]]);
                self.push(0x88, vec![0x31, 0, 0]);
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
}
