//! Renesas RX firmware writer for the documented DU-EP800 family.
//!
//! SM-PCE02 uses the A8 command envelope and control11 buffered60-byte
//! transfers. It must not be mixed with the retained control48/0B legacy path.
//!
//! RX update entry changes the MCU's operating mode. Even `read_boot` enters
//! firmware mode; it is not a normal application identity read. Never repeat an
//! erase automatically after an I/O error: reconcile the device state first.
use crate::firmware_io::FirmwareIo;
use crate::platform::sleep;
use crate::{
    device::Session,
    firmware::{Firmware, FirmwareKind, Version},
    protocol::Frame,
};
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use std::{collections::VecDeque, time::Duration};

const BASE: usize = 0xfc0000;
const SECTOR: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    Legacy,
    Smpce02,
}

#[derive(Clone, Debug, Serialize)]
pub struct Boot {
    pub transport: Transport,
    pub version: Version,
    pub capability_sequence: u8,
    pub retry_capable: bool,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub bytes: usize,
    pub checksum: u8,
    pub boot: Boot,
    pub finish_acknowledged: bool,
    pub reset_acknowledged: bool,
}

/// Requires an already established normal PC-link session at `slot`. The
/// selector switches RX into update mode, then prepares the DCAS fast bridge.
/// This changes both operating modes; no erase or image data is sent here.
/// Complete the probe with `reset_fast` before starting another normal session.
pub fn read_boot(session: &mut Session, slot: u8) -> Result<Boot> {
    ensure!(slot < 32, "invalid RX application slot {slot}");
    session.target(slot)?;
    // Preserve the assigned address across the application-to-boot jump.
    // Send the update preamble before mode selection and keys.
    session
        .command(slot, 1, 0xc0, &[slot], 0xc2)
        .context("retain discovered motor slot across boot entry")?;
    session
        .command(slot, 0x32, 0x20, &[1], 0x22)
        .context("enter RX update mode (no erase)")?;
    let assigned_slot = session
        .fast_prepare(slot)
        .context("prepare DCAS fast RX boot bridge (no erase)")?;
    ensure!(
        assigned_slot == slot,
        "fast bridge changed slot unexpectedly"
    );
    sleep(Duration::from_millis(100));
    let f = fast(session, slot, 0x41, [0, 0, 0], 0x51, 3)?;
    let p = &f.payload;
    ensure!(p.len() >= 9, "truncated fast RX bootloader reply");
    let version = Version::packed(&p[6..9])?;
    ensure!(version.0[0] != 0, "RX bootloader version is zero");
    ensure!(p[4] <= 1, "unsupported RX capability sequence {}", p[4]);
    Ok(Boot {
        transport: Transport::Smpce02,
        version,
        capability_sequence: p[4],
        retry_capable: p[4] == 1,
    })
}

/// Programs the entire M image, including its header, at the RX DU base address.
/// Every checksum window is acknowledged before progress advances. Completion
/// remains subject to a separate normal-session version/identity readback.
pub fn flash(
    session: &mut Session,
    slot: u8,
    image: &Firmware,
    progress: impl FnMut(usize, usize),
) -> Result<Receipt> {
    flash_with_policy(session, slot, image, false, progress)
}

pub fn flash_with_policy(
    session: &mut Session,
    slot: u8,
    image: &Firmware,
    allow_unverified_model: bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Receipt> {
    ensure!(
        image.metadata.kind == FirmwareKind::Renesas,
        "RX requires a plaintext M/Renesas image"
    );
    if !allow_unverified_model {
        ensure!(
            image.metadata.series == 0x23 && image.metadata.unit == 0,
            "RX writer supports only the documented DU-EP800 family 23/00 without --allow-unverified-model"
        );
    } else {
        ensure!(
            crate::firmware::is_supported_rx_target(image.metadata.series, image.metadata.unit),
            "RX writer does not support firmware family {:02X}/{:02X}",
            image.metadata.series,
            image.metadata.unit
        );
    }
    ensure!(!image.data.is_empty(), "empty RX image");
    ensure!(
        BASE.checked_add(image.data.len().div_ceil(SECTOR) * SECTOR)
            .is_some_and(|end| end <= 0x1000000),
        "RX image exceeds its 24-bit application address range"
    );
    let boot = read_boot(session, slot)?;
    if boot.transport == Transport::Smpce02 {
        return flash_fast(session, slot, image, boot, allow_unverified_model, &mut progress);
    }
    // All capability and size checks above precede this destructive start.
    legacy(session, 0x21, [0, 0x0e, 0], 0x31, 6)
        .context("RX erase/start failed; outcome uncertain, do not rerun automatically")?;
    let next_packet_id = if boot.retry_capable {
        Some(transfer_retry(session, &image.data, &mut progress)?)
    } else {
        let window = if boot.version >= Version([2, 0, 2, 0]) {
            1024
        } else {
            64
        };
        transfer_ordinary(session, &image.data, window, &mut progress)?;
        None
    };
    // Both algorithms finish once. The retry branch additionally restores the
    // adapter's command-length mode after the finish acknowledgement.
    legacy(session, 0x27, [image.metadata.checksum, 0, 0], 0x31, 21)
        .context("RX finish uncertain; reconcile before another erase")?;
    if let Some(id) = next_packet_id {
        adapter_command(session, &[id, 0x04, 2], 0x84, id, 3)?;
    }
    let reset_acknowledged =
        reset(session).context("RX image finished but reset acknowledgement is uncertain")?;
    Ok(Receipt {
        bytes: image.data.len(),
        checksum: image.metadata.checksum,
        boot,
        finish_acknowledged: true,
        reset_acknowledged,
    })
}

/// Explicitly reset RX after a capability-only probe when returning to the app.
pub fn reset(session: &mut Session) -> Result<bool> {
    legacy(session, 0x28, [0, 0, 0], 0x31, 6)?;
    sleep(Duration::from_secs(1));
    Ok(true)
}

/// Return the fast boot bridge to the normal application. The modern
/// updater passes isSendReset=false to its RX writer and resets the surrounding
/// DCAS session instead. An RX-specific28 is not reliably acknowledged on the
/// measured2.0.7.2 bootloader; final RX status requires normal application readback.
pub fn reset_fast(session: &mut Session, slot: u8) -> Result<bool> {
    session
        .fast_finish(slot)
        .context("restore DCAS application after RX session")?;
    sleep(Duration::from_secs(1));
    Ok(true)
}
fn fast_payload(slot: u8, cmd: u8, params: [u8; 3]) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[..9].copy_from_slice(&[slot, 0x60, 0xf0, 5, 0, cmd, params[0], params[1], params[2]]);
    p
}
fn fast(
    session: &mut Session,
    slot: u8,
    cmd: u8,
    params: [u8; 3],
    expected: u8,
    timeout: u64,
) -> Result<Frame> {
    let f = session
        .exchange(
            0xa8,
            &fast_payload(slot, cmd, params),
            Duration::from_secs(timeout),
            |f| {
                (f.control == 0xa8
                    && f.payload.len() >= 16
                    && f.payload.starts_with(&[slot, 0x70, 0xf2])
                    && (f.payload[5] == expected || f.payload[5] == 0x32))
                    || (f.control == 0x48 && f.payload.get(1..3) == Some(&[0x60, 0xf3]))
            },
        )
        .with_context(|| format!("RX fast command {cmd:02X}; outcome may be uncertain"))?;
    if f.control == 0x48 {
        bail!(
            "RX fast envelope rejected by DCAS application: {} (RX boot bridge is unavailable)",
            hex::encode(&f.payload)
        );
    }
    if f.payload[5] == 0x32 {
        bail!(
            "RX fast command {cmd:02X} rejected: error {} ({})",
            f.payload[6],
            hex::encode(&f.payload)
        );
    }
    Ok(f)
}

fn fast_data_payload(packet: u8, sequence: u8, slot: u8, data: &[u8]) -> Vec<u8> {
    assert!(data.len() <= 60);
    let mut p = vec![0xff; 69];
    p[..5].copy_from_slice(&[packet, 0x11, sequence, 0xf8, slot]);
    p[5..5 + data.len()].copy_from_slice(data);
    p
}
fn fast_sequence(index: usize) -> u8 {
    ((index % 43) * 6) as u8
}
fn fast_transfer_reply(frame: &Frame, slot: u8, response: u8, packet_id: u8) -> bool {
    matches!(frame.control, 0x11 | 0x31)
        && frame.payload.len() > 4
        && frame.payload[1] == response
        && frame.payload[2] == slot
        && frame.payload[3] == packet_id
}

fn validate_fast_capabilities(boot: &Boot, allow_unverified_model: bool) -> Result<()> {
    ensure!(
        !boot.retry_capable || allow_unverified_model,
        "retry-capable RX fast bootloader is not hardware validated; refusing before erase (pass --allow-unverified-model to authorize)"
    );
    Ok(())
}

fn flash_fast(
    session: &mut Session,
    slot: u8,
    image: &Firmware,
    boot: Boot,
    allow_unverified_model: bool,
    progress: &mut impl FnMut(usize, usize),
) -> Result<Receipt> {
    validate_fast_capabilities(&boot, allow_unverified_model)?;
    // Both protocol variants share the transfer wrapper and erase value. The
    // retry capability changes the checksum block from1080 to2100 bytes.
    let block_len = if boot.retry_capable { 2100 } else { 1080 };
    ensure!(
        BASE + image.data.len().div_ceil(60) * 60 <= 0x1000000,
        "padded fast RX image exceeds address range"
    );
    fast(session, slot, 0x21, [0, 0x0e, 0], 0x31, 6)
        .context("RX fast erase/start uncertain; do not repeat automatically")?;
    let a = BASE;
    fast(
        session,
        slot,
        0x24,
        [a as u8, (a >> 8) as u8, (a >> 16) as u8],
        0x31,
        10,
    )?;
    fast(session, slot, 0x26, [0, 0, 0], 0x31, 2)?;
    let mut counter = PacketCounter::default();
    for offset in (0..image.data.len()).step_by(block_len) {
        if boot.retry_capable && offset > 0 {
            let address = BASE + offset;
            fast(
                session,
                slot,
                0x24,
                [address as u8, (address >> 8) as u8, (address >> 16) as u8],
                0x31,
                10,
            )?;
            fast(session, slot, 0x26, [0, 0, 0], 0x31, 2)?;
        }
        let real_end = (offset + block_len).min(image.data.len());
        let block = &image.data[offset..real_end];
        let mut partial = 0u8;
        for (index, chunk) in block.chunks(60).enumerate() {
            let id = counter.next();
            let p = fast_data_payload(id, fast_sequence(index), slot, chunk);
            // The partial checksum covers the60-byte fragment including EOF
            // FF padding, but excludes the four trailing adapter padding bytes.
            partial = partial.wrapping_add(sum(&p[5..65]));
            let loaded = session.exchange(0x11, &p, Duration::from_secs(2), |f| {
                matches!(f.control, 0x11 | 0x31)
                    && f.payload.get(1) == Some(&0x91)
                    && f.payload.get(2) == Some(&slot)
                    && f.payload.len() > 4
            })?;
            ensure!(
                loaded.payload[4] == 0,
                "RX fast data buffer rejected: {}",
                hex::encode(&loaded.payload)
            );
            let transfer_id = counter.next();
            // Hardware sends an intermediate83 acceptance and a separateC0
            // completion. Consume both; leaving83 queued over2137 fragments
            // otherwise exhausts the unsolicited-frame queue near122700bytes.
            // C0 may arrive first: exchange preserves it for the followingwait.
            let accepted = session
                .exchange(0x11, &[transfer_id, 3, id], Duration::from_secs(2), |f| {
                    fast_transfer_reply(f, slot, 0x83, transfer_id)
                })
                .with_context(|| {
                    format!(
                        "RX fast transfer acceptance at byte {} uncertain",
                        offset + index * 60
                    )
                })?;
            ensure!(
                accepted.payload[4] == 0,
                "RX fast transfer rejected: {}",
                hex::encode(&accepted.payload)
            );
            let done = session
                .wait(Duration::from_secs(2), |f| {
                    fast_transfer_reply(f, slot, 0xc0, id)
                })
                .with_context(|| {
                    format!(
                        "RX fast transfer completion at byte {} uncertain",
                        offset + index * 60
                    )
                })?;
            ensure!(
                done.payload[4] == 0,
                "RX fast transfer failed: {}",
                hex::encode(&done.payload)
            );
        }
        fast(session, slot, 0x25, [partial, 0, 0], 0x31, 3)
            .with_context(|| format!("RX fast checksum block at {offset} unverified"))?;
        fast(session, slot, 0x26, [0, 0, 0], 0x31, 2)?;
        progress(real_end, image.data.len());
    }
    fast(
        session,
        slot,
        0x27,
        [image.metadata.checksum, 0, 0],
        0x31,
        21,
    )
    .context("RX fast finish uncertain; reconcile before another erase")?;
    reset_fast(session, slot).context("RX fast finish acknowledged but reset uncertain")?;
    Ok(Receipt {
        bytes: image.data.len(),
        checksum: image.metadata.checksum,
        boot,
        finish_acknowledged: true,
        reset_acknowledged: true,
    })
}

/// BLE uses the stock unaccelerated F0/F2 bridge in the running DCAS app.
/// The same application relay can be carried by ordinary SM-PCE02 control48
/// records without entering its separate 2MHz boot bridge.
pub struct NormalBridge<'a> {
    session: &'a mut Session,
    slot: u8,
}
impl<'a> NormalBridge<'a> {
    pub fn enter(session: &'a mut Session, slot: u8) -> Result<(Self, Boot)> {
        session.target(slot)?;
        session.command(slot, 1, 0xc0, &[slot], 0xc2)?;
        session.command(slot, 0x32, 0x20, &[1], 0x22)?;
        sleep(Duration::from_secs(1));
        let mut link = Self { session, slot };
        let boot = read_legacy_boot(&mut link)?;
        Ok((link, boot))
    }
    pub fn reset(&mut self) -> Result<()> {
        reset_legacy(self)
    }
    pub fn flash(
        &mut self,
        image: &Firmware,
        boot: Boot,
        progress: impl FnMut(usize, usize),
    ) -> Result<Receipt> {
        flash_legacy(self, image, boot, progress)
    }
}
impl FirmwareIo for NormalBridge<'_> {
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        ensure!(
            control == 0x48 && payload.len() == 7 && matches!(payload[1], 0xf0 | 0xf8),
            "unsupported normal RX relay record"
        );
        let mut p = payload.to_vec();
        p[0] = self.slot;
        self.session.send(control, &p)
    }
    fn wait(&mut self, timeout: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
        let pc_slot = self.session.normal_pc_slot();
        self.session.wait(timeout, |f| {
            f.control == 0x48 && f.payload.first() == Some(&pc_slot) && check(f)
        })
    }
    fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool) {
        FirmwareIo::discard_matching(self.session, check);
    }
}

pub(crate) fn read_legacy_boot(session: &mut impl FirmwareIo) -> Result<Boot> {
    let f = legacy(session, 0x41, [0, 0, 0], 0x51, 6)?;
    let p = legacy_body(&f).unwrap();
    ensure!(p.len() >= 6, "truncated RX legacy capability/version");
    ensure!(
        p[1] == 0,
        "RX retry-capable BLE bootloader is not validated; refusing before erase"
    );
    let version = Version::packed(&p[3..6])?;
    ensure!(version.0[0] != 0, "zero RX bootloader version");
    Ok(Boot {
        transport: Transport::Legacy,
        version,
        capability_sequence: p[1],
        retry_capable: false,
    })
}
pub(crate) fn reset_legacy(session: &mut impl FirmwareIo) -> Result<()> {
    legacy(session, 0x28, [0, 0, 0], 0x31, 6)?;
    sleep(Duration::from_secs(1));
    Ok(())
}
pub(crate) fn flash_legacy(
    session: &mut impl FirmwareIo,
    image: &Firmware,
    boot: Boot,
    mut progress: impl FnMut(usize, usize),
) -> Result<Receipt> {
    ensure!(
        boot.transport == Transport::Legacy && !boot.retry_capable,
        "unsupported BLE RX bootloader capability"
    );
    ensure!(
        image.metadata.kind == FirmwareKind::Renesas
            && crate::firmware::is_supported_rx_target(image.metadata.series, image.metadata.unit),
        "BLE RX requires a supported drive unit M firmware"
    );
    ensure!(
        !image.data.is_empty() && image.data.len().div_ceil(SECTOR) * SECTOR <= 0x40000,
        "invalid RX image length"
    );
    legacy(session, 0x21, [0, 0x0e, 0], 0x31, 6)
        .context("RX erase uncertain; automatic retries disabled")?;
    let window = if boot.version >= Version([2, 0, 2, 0]) {
        1024
    } else {
        64
    };
    transfer_ordinary(session, &image.data, window, &mut progress)?;
    legacy(session, 0x27, [image.metadata.checksum, 0, 0], 0x31, 21)?;
    reset_legacy(session)?;
    Ok(Receipt {
        bytes: image.data.len(),
        checksum: image.metadata.checksum,
        boot,
        finish_acknowledged: true,
        reset_acknowledged: true,
    })
}

fn legacy_body(f: &Frame) -> Option<&[u8]> {
    if f.control != 0x48 {
        return None;
    }
    // The physical control48 reply retains the leading adapter routing byte;
    // parse the remaining F2 envelope after the routing byte.
    let p = f.payload.get(1..)?;
    (p.first() == Some(&0xf2)).then_some(p)
}
fn legacy(
    session: &mut impl FirmwareIo,
    cmd: u8,
    params: [u8; 3],
    expected: u8,
    timeout: u64,
) -> Result<Frame> {
    let payload = [0, 0xf0, 0, cmd, params[0], params[1], params[2]];
    let f = session
        .exchange(0x48, &payload, Duration::from_secs(timeout), |f| {
            legacy_body(f).is_some_and(|p| {
                p.len() >= 3
                    && (p[2] == expected || p[2] == 0x32)
                    && (p[1] == 0 || cmd == 0x41 && p[1] == 1)
            })
        })
        .with_context(|| format!("RX command {cmd:02X}; outcome may be uncertain"))?;
    let p = legacy_body(&f).unwrap();
    if p[2] == 0x32 {
        bail!(
            "RX command {cmd:02X} rejected: error {} ({})",
            p.get(3).copied().unwrap_or(255),
            hex::encode(p)
        );
    }
    Ok(f)
}
fn address(session: &mut impl FirmwareIo, offset: usize) -> Result<()> {
    let a = BASE + offset;
    ensure!(a <= 0xffffff, "RX write address out of range");
    legacy(
        session,
        0x24,
        [a as u8, (a >> 8) as u8, (a >> 16) as u8],
        0x31,
        10,
    )?;
    legacy(session, 0x26, [0, 0, 0], 0x31, 2)?;
    Ok(())
}
fn sum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
}
fn padded_window(data: &[u8], offset: usize, length: usize) -> Vec<u8> {
    let mut window = vec![0xff; length];
    let real = (data.len() - offset).min(length);
    window[..real].copy_from_slice(&data[offset..offset + real]);
    window
}
fn transfer_ordinary(
    session: &mut impl FirmwareIo,
    data: &[u8],
    window_len: usize,
    progress: &mut impl FnMut(usize, usize),
) -> Result<()> {
    address(session, 0)?;
    let mut sequence = 0u8;
    for offset in (0..data.len()).step_by(window_len) {
        // Stop at the final 64-byte packet in the
        // image. Only that packet is padded, never the rest of a 1 KiB window.
        let packet_bytes = (data.len() - offset).min(window_len).div_ceil(64) * 64;
        let window = padded_window(data, offset, packet_bytes);
        for bytes in window.chunks_exact(4) {
            sequence = (sequence + 1) % 8;
            session.send(
                0x48,
                &[0, 0xf8, sequence, bytes[0], bytes[1], bytes[2], bytes[3]],
            )?;
            // Conservative adapter pacing also applies to pre-speedup BLs.
            session.pace(Duration::from_millis(2));
        }
        legacy(session, 0x25, [sum(&window), 0, 0], 0x31, 2).with_context(|| {
            format!("RX window at {offset} unverified; do not restart erase automatically")
        })?;
        legacy(session, 0x26, [0, 0, 0], 0x31, 2)?;
        progress((offset + window_len).min(data.len()), data.len());
    }
    Ok(())
}
#[derive(Default)]
struct PacketCounter(u16);
impl PacketCounter {
    fn next(&mut self) -> u8 {
        let id = self.0 as u8;
        self.0 = (self.0 + 1) % 255;
        id
    }
}
fn adapter_command(
    session: &mut Session,
    payload: &[u8],
    reply: u8,
    id: u8,
    timeout: u64,
) -> Result<()> {
    let f = session.exchange(0x0b, payload, Duration::from_secs(timeout), |f| {
        f.control & 0x0b == 0x0b
            && f.payload.get(1) == Some(&reply)
            && f.payload.get(2) == Some(&id)
    })?;
    ensure!(
        f.payload.get(3) == Some(&0),
        "RX adapter command rejected: {}",
        hex::encode(f.payload)
    );
    Ok(())
}
fn completion(session: &mut Session, id: u8, timeout: u64) -> Result<()> {
    session
        .wait(Duration::from_secs(timeout), |f| {
            f.control & 0x0b == 0x0b
                && f.payload.get(1) == Some(&0xc0)
                && f.payload.get(2) == Some(&id)
        })
        .with_context(|| format!("RX buffer {id} transfer completion uncertain"))?;
    Ok(())
}
fn transfer_retry(
    session: &mut Session,
    data: &[u8],
    progress: &mut impl FnMut(usize, usize),
) -> Result<u8> {
    let mut packets = PacketCounter::default();
    for offset in (0..data.len()).step_by(SECTOR) {
        address(session, offset)?;
        let mode_id = packets.next();
        adapter_command(session, &[mode_id, 0x04, 1], 0x84, mode_id, 3)?;
        let sector = padded_window(data, offset, SECTOR);
        let mut pending = VecDeque::new();
        for (index, bytes) in sector.chunks_exact(64).enumerate() {
            let buffer_id = packets.next();
            let sequence = ((index % 16) * 16) as u8;
            let mut p = vec![buffer_id, 0x11, sequence];
            p.extend_from_slice(bytes);
            adapter_command(session, &p, 0x91, buffer_id, 2)?;
            let transfer_id = packets.next();
            adapter_command(
                session,
                &[transfer_id, 0x03, buffer_id],
                0x83,
                transfer_id,
                2,
            )?;
            pending.push_back(buffer_id);
            // Keep the two-buffer pipeline. C0 may precede its 83 ACK;
            // Session queues unmatched frames so neither ordering loses it.
            if pending.len() >= 2 {
                completion(
                    session,
                    pending.pop_front().unwrap(),
                    if index < 2 { 25 } else { 3 },
                )?;
            }
        }
        while let Some(id) = pending.pop_front() {
            completion(session, id, 3)?;
        }
        legacy(session, 0x25, [sum(&sector), 0, 0], 0x31, 3).with_context(|| {
            format!("RX sector at {offset} unverified; automatic retries disabled")
        })?;
        legacy(session, 0x26, [0, 0, 0], 0x31, 2)?;
        progress((offset + SECTOR).min(data.len()), data.len());
    }
    Ok(packets.next())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct LegacyReplay {
        pending: VecDeque<Frame>,
        bytes: Vec<u8>,
        sequence: u8,
        window: Vec<u8>,
        commands: Vec<u8>,
        reject_checksum: bool,
    }
    impl FirmwareIo for LegacyReplay {
        fn pace(&mut self, _: Duration) {}
        fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool) {
            self.pending.retain(|f| !check(f));
        }
        fn wait(&mut self, _: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
            let index = self
                .pending
                .iter()
                .position(check)
                .context("missing RX fixture reply")?;
            Ok(self.pending.remove(index).unwrap())
        }
        fn send(&mut self, control: u8, p: &[u8]) -> Result<()> {
            assert_eq!(control, 0x48);
            assert_eq!(p.len(), 7);
            if p[1] == 0xf8 {
                self.sequence = (self.sequence + 1) % 8;
                assert_eq!(p[2], self.sequence);
                self.bytes.extend_from_slice(&p[3..]);
                self.window.extend_from_slice(&p[3..]);
                return Ok(());
            }
            assert_eq!(p[1], 0xf0);
            self.commands.push(p[3]);
            if p[3] == 0x24 {
                assert_eq!(&p[4..], &[0, 0, 0xfc]);
            }
            if p[3] == 0x25 {
                assert_eq!(p[4], sum(&self.window));
                assert!(self.window.len() <= 1024 && self.window.len().is_multiple_of(64));
            }
            if p[3] == 0x26 {
                self.window.clear();
            }
            let response = if self.reject_checksum && p[3] == 0x25 {
                vec![0, 0xf2, 0, 0x32, 7]
            } else {
                vec![0, 0xf2, 0, 0x31]
            };
            self.pending.push_back(Frame {
                control: 0x48,
                payload: response,
            });
            Ok(())
        }
    }
    fn legacy_fixture(size: usize) -> (LegacyReplay, Firmware, Boot) {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let image = Firmware {
            metadata: crate::firmware::FirmwareMetadata {
                kind: FirmwareKind::Renesas,
                version: Version([4, 1, 11, 0]),
                compatibility_version: Version([4, 1, 9, 0]),
                series: 0x23,
                unit: 0,
                board: None,
                encrypted: false,
                source_bytes: data.len(),
                decoded_bytes: data.len(),
                source_sha256: String::new(),
                decoded_sha256: String::new(),
                checksum: sum(&data),
            },
            data,
        };
        let replay = LegacyReplay {
            pending: VecDeque::new(),
            bytes: Vec::new(),
            sequence: 0,
            window: Vec::new(),
            commands: Vec::new(),
            reject_checksum: false,
        };
        let boot = Boot {
            transport: Transport::Legacy,
            version: Version([2, 0, 7, 2]),
            capability_sequence: 0,
            retry_capable: false,
        };
        (replay, image, boot)
    }
    #[test]
    fn complete_ble_rx_stream_preserves_image_padding_sequence_and_window_checksums() {
        let (mut replay, image, boot) = legacy_fixture(128192);
        let mut done = 0;
        let receipt = flash_legacy(&mut replay, &image, boot, |n, _| done = n).unwrap();
        assert_eq!(done, image.data.len());
        assert!(receipt.finish_acknowledged && receipt.reset_acknowledged);
        assert_eq!(&replay.bytes[..image.data.len()], &image.data);
        assert!(replay.bytes[image.data.len()..].iter().all(|b| *b == 255));
        assert_eq!(replay.bytes.len(), image.data.len().div_ceil(64) * 64);
        assert_eq!(replay.commands.iter().filter(|c| **c == 0x21).count(), 1);
        assert_eq!(
            replay.commands.iter().filter(|c| **c == 0x25).count(),
            image.data.len().div_ceil(1024)
        );
        assert_eq!(&replay.commands[replay.commands.len() - 2..], &[0x27, 0x28]);
    }
    #[test]
    fn final_rx_window_stops_after_last_64_byte_packet() {
        for size in [65, 1025, 128191] {
            let (mut replay, image, boot) = legacy_fixture(size);
            flash_legacy(&mut replay, &image, boot, |_, _| {}).unwrap();
            assert_eq!(replay.bytes.len(), size.div_ceil(64) * 64);
            assert_eq!(&replay.bytes[..size], image.data);
            assert!(replay.bytes[size..].iter().all(|b| *b == 255));
        }
    }
    #[test]
    fn rx_checksum_rejection_aborts_before_more_data_finish_or_reset() {
        let (mut replay, image, boot) = legacy_fixture(128192);
        replay.reject_checksum = true;
        assert!(flash_legacy(&mut replay, &image, boot, |_, _| {}).is_err());
        assert_eq!(replay.bytes.len(), 1024);
        assert_eq!(replay.commands.iter().filter(|c| **c == 0x21).count(), 1);
        assert!(!replay.commands.iter().any(|c| matches!(c, 0x27 | 0x28)));
    }
    #[test]
    fn fast_vectors_match_transfer_envelopes() {
        assert_eq!(
            fast_payload(13, 0x41, [0, 0, 0]),
            [13, 0x60, 0xf0, 5, 0, 0x41, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        let p = fast_data_payload(7, 12, 13, &[1, 2]);
        assert_eq!(p.len(), 69);
        assert_eq!(&p[..7], &[7, 0x11, 12, 0xf8, 13, 1, 2]);
        assert!(p[7..].iter().all(|b| *b == 0xff));
        assert_eq!(sum(&p[5..65]), 3u8.wrapping_sub(58));
        assert_eq!(
            (0..45).map(fast_sequence).collect::<Vec<_>>(),
            (0..45).map(|i| ((i % 43) * 6) as u8).collect::<Vec<_>>()
        );
    }
    #[test]
    fn full_image_transfer_consumes_ack_and_early_or_late_completion() {
        // Recorded hardware shapes: F8 83 0D 0B 00, F9 C0 0D 0A 00.
        // Replay more than the2048 unsolicited-frame cap, including packetID
        // wrap and both possible receive orders. Every response must be drained.
        for early_completion in [false, true] {
            let mut queue = VecDeque::new();
            let mut ids = PacketCounter::default();
            for i in 0..128192usize.div_ceil(60) {
                let data_id = ids.next();
                let transfer_id = ids.next();
                let ack = Frame {
                    control: 0x31,
                    payload: vec![0xf8, 0x83, 13, transfer_id, 0],
                };
                let done = Frame {
                    control: 0x31,
                    payload: vec![0xf9, 0xc0, 13, data_id, 0],
                };
                if early_completion {
                    queue.push_back(done);
                    queue.push_back(ack);
                } else {
                    queue.push_back(ack);
                    queue.push_back(done);
                }
                let ack = queue
                    .remove(
                        queue
                            .iter()
                            .position(|f| fast_transfer_reply(f, 13, 0x83, transfer_id))
                            .unwrap(),
                    )
                    .unwrap();
                assert_eq!(ack.payload[4], 0);
                let done = queue
                    .remove(
                        queue
                            .iter()
                            .position(|f| fast_transfer_reply(f, 13, 0xc0, data_id))
                            .unwrap(),
                    )
                    .unwrap();
                assert_eq!(done.payload[4], 0);
                assert!(queue.is_empty(), "response leaked at fragment{i}");
            }
        }
        let frame = Frame {
            control: 0x31,
            payload: vec![0xf8, 0x83, 13, 11, 1],
        };
        assert!(fast_transfer_reply(&frame, 13, 0x83, 11)); // Caller validates rejection status.
        assert!(!fast_transfer_reply(&frame, 12, 0x83, 11));
        assert!(!fast_transfer_reply(&frame, 13, 0x83, 10));
        assert!(!fast_transfer_reply(&frame, 13, 0xc0, 11));
    }
    #[test]
    fn retry_capable_fast_bootloader_refused_before_write() {
        let mut boot = Boot {
            transport: Transport::Smpce02,
            version: Version([2, 0, 7, 2]),
            capability_sequence: 0,
            retry_capable: false,
        };
        validate_fast_capabilities(&boot, false).unwrap();
        boot.capability_sequence = 1;
        boot.retry_capable = true;
        assert!(validate_fast_capabilities(&boot, false)
            .unwrap_err()
            .to_string()
            .contains("before erase"));
        validate_fast_capabilities(&boot, true).unwrap();
    }
    #[test]
    fn eof_checksum_includes_padding_only_in_window() {
        let image = vec![1u8; 128192];
        let window = padded_window(&image, 126976, 2048);
        assert_eq!(window.iter().filter(|&&b| b == 0xff).count(), 832);
        assert_eq!(sum(&image), 192);
        assert_eq!(sum(&window), 128);
    }
    #[test]
    fn packet_ids_never_use_255() {
        let mut p = PacketCounter::default();
        assert_eq!(
            (0..257).map(|_| p.next()).collect::<Vec<_>>(),
            (0..257).map(|i| (i % 255) as u8).collect::<Vec<_>>()
        );
    }
    #[test]
    fn legacy_reply_requires_control_and_envelope() {
        assert!(legacy_body(&Frame {
            control: 0x48,
            payload: vec![0, 0xf2, 1, 0x51, 0x20, 2, 0]
        })
        .is_some());
        assert!(legacy_body(&Frame {
            control: 0x48,
            payload: vec![0, 0xff, 1, 0x51]
        })
        .is_none());
        assert!(legacy_body(&Frame {
            control: 0x0b,
            payload: vec![0, 0xf2]
        })
        .is_none());
    }
}
