//! Motor firmware relay over PCE_COMMAND. Never opens the gateway radio's BOOT characteristic.
use super::*;
use crate::{
    device::{BootIdentity, FlashReceipt},
    firmware_io::{self, FirmwareIo},
    protocol::Frame,
};
use std::collections::VecDeque;

/// Holds exclusive ownership of logical update traffic until application return.
pub struct MotorFirmware<'a> {
    session: &'a mut BleSession,
    slot: u8,
    incoming: VecDeque<Frame>,
    rx_mode: bool,
    dcas_mode: bool,
}

/// Fragmented writes use 18 payload bytes, repeated control and sequence
/// using 0x20 sequence steps. Short commands use a zero prefix.
fn pce_fragments(control: u8, payload: &[u8]) -> Result<Vec<Vec<u8>>> {
    ensure!(
        payload.len() < 72,
        "motor relay message exceeds stock BLE limit"
    );
    if payload.len() + 1 <= 18 {
        let mut value = vec![0, control];
        value.extend_from_slice(payload);
        return Ok(vec![value]);
    }
    // Count the logical control in the remaining length. When the
    // payload is exactly divisible by 18 it emits an empty final record.
    let count = (payload.len() + 1).div_ceil(18);
    Ok((0..count)
        .map(|i| {
            let chunk = &payload[(i * 18).min(payload.len())..((i + 1) * 18).min(payload.len())];
            let more = i + 1 < count;
            let mut value = vec![(i as u8 * 32) | if more { 0x80 } else { 0 }, control];
            value.extend_from_slice(chunk);
            value
        })
        .collect())
}
impl BleSession {
    /// Continue only the M half of a locally journaled, acknowledged D write.
    /// This does not admit an unknown interrupted D write or a previous M erase.
    pub fn continue_pair_rx(
        &mut self,
        expected_serial: &[u8; 6],
        dcas: &crate::firmware::Firmware,
        rx: &crate::firmware::Firmware,
        previous_journal: &Path,
    ) -> Result<MotorFirmware<'_>> {
        dcas.validate_companion(rx)?;
        let bytes = crate::storage::read_bounded(previous_journal, 1024 * 1024)?;
        let records: Vec<serde_json::Value> = String::from_utf8(bytes)?
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        let stage = |name: &str| -> Result<&serde_json::Value> {
            let matches: Vec<_> = records.iter().filter(|r| r["stage"] == name).collect();
            ensure!(
                matches.len() == 1,
                "continuation requires exactly one {name} receipt"
            );
            Ok(&matches[0]["data"])
        };
        let inputs = stage("inputs")?;
        ensure!(
            inputs["write"] == true
                && inputs["serial"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&hex::encode(expected_serial))),
            "prior write serial mismatch"
        );
        ensure!(
            inputs["dcas"] == serde_json::to_value(&dcas.metadata)?
                && inputs["rx"] == serde_json::to_value(&rx.metadata)?,
            "continuation image metadata differs from original pair"
        );
        let boot = stage("dcas-boot-before")?;
        ensure!(
            boot["serial"] == serde_json::to_value(expected_serial)?
                && boot["series"] == 35
                && boot["unit"] == 0
                && boot["board"] == 1,
            "prior independent boot identity mismatch"
        );
        let finished = stage("dcas-finish-and-reset")?;
        ensure!(
            finished["bytes"] == dcas.data.len()
                && finished["checksum"] == dcas.metadata.checksum
                && finished["finish_acknowledged"] == true
                && finished["reset_acknowledged"] == true,
            "D programming/reset is not confirmed"
        );
        ensure!(
            !records.iter().any(|r| matches!(
                r["stage"].as_str(),
                Some("rx-destructive-start" | "rx-finish-and-reset" | "verified")
            )),
            "M erase already attempted or pair already completed; no automatic replay"
        );
        let backup = stage("settings-backup")?;
        let saved: serde_json::Value = serde_json::from_slice(&crate::storage::read_bounded(
            Path::new(backup["path"].as_str().context("missing original backup")?),
            1024 * 1024,
        )?)?;
        ensure!(
            saved["serial"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(&hex::encode(expected_serial)))
                && saved["unavailable"]
                    .as_object()
                    .is_some_and(|m| m.is_empty())
                && saved["values"].as_object().is_some_and(|m| !m.is_empty()),
            "original serial-bound full backup is unavailable"
        );
        // A mismatched pair can explicitly reject the application version
        // request with 39. No other setup error is admitted by this path.
        if let Err(error) = self.start_component(None, None) {
            let slot = self
                .linked_slot
                .context("motor PC link was not established")?;
            ensure!(
                error
                    .to_string()
                    .contains(&format!("group 01 command 2C: reply {slot:02x}012f39")),
                "continuation application start failed: {error:#}"
            );
        }
        ensure!(
            self.authenticated
                && self.gateway_mode
                && self.master_routing_active
                && !self.poisoned
                && !self.firmware_active,
            "continuation requires a healthy authenticated master connection"
        );
        let slot = self.linked_slot.context("no selected continuation motor")?;
        let stock = self.model_identity(slot)?;
        ensure!(
            stock.get(1..3) == Some(&[35, 0]),
            "continuation requires EP800"
        );
        let serial = self.command(slot, 1, 0x3c, &[0], 0x3e)?;
        ensure!(
            &crate::device::serial_bytes(&serial)? == expected_serial,
            "continuation live serial mismatch"
        );
        let cargo = self.command(slot, 0x16, 0x7c, &[0], 0x7e)?;
        ensure!(
            cargo.as_slice() == b"00-CRG\0",
            "continuation cargo mismatch"
        );
        ensure!(
            [PCE_COMMAND, FREE_COMMAND].iter().all(|id| self
                .chars
                .get(id)
                .is_some_and(|c| c.write || c.write_without_response)),
            "continuation relay is not writable"
        );
        self.drain()?;
        self.send_unit(slot, 1, 0xc0, &[slot])?;
        self.wait(|n| {
            n.characteristic == FREE_RESPONSE && n.value.starts_with(&[slot, 1, 0xc2, slot])
        })?;
        self.drain()?;
        self.firmware_active = true;
        Ok(MotorFirmware {
            session: self,
            slot,
            incoming: VecDeque::new(),
            rx_mode: false,
            dcas_mode: false,
        })
    }
    /// Gate motor entry on a fresh normal session and independent identity.
    /// The returned relay cannot address a gateway BOOT characteristic.
    pub fn motor_firmware(&mut self, expected_serial: &[u8; 6]) -> Result<MotorFirmware<'_>> {
        ensure!(
            self.authenticated && self.gateway_mode && !self.firmware_active,
            "start a normal BLE motor session first"
        );
        ensure!(
            self.master_routing && self.master_routing_active,
            "BLE motor firmware requires master routing"
        );
        let slot = self.linked_slot.context("no selected motor")?;
        ensure!(
            [PCE_COMMAND, FREE_COMMAND].iter().all(|id| self
                .chars
                .get(id)
                .is_some_and(|c| c.write || c.write_without_response)),
            "BLE firmware requires writable relay characteristics: PCE={:?}, FREE={:?}",
            self.chars.get(&PCE_COMMAND),
            self.chars.get(&FREE_COMMAND)
        );
        let stock = self.model_identity(slot)?;
        ensure!(
            stock.get(1..3) == Some(&[0x23, 0]),
            "BLE firmware supports only EP800 23/00"
        );
        let serial = self.command(slot, 1, 0x3c, &[0], 0x3e)?;
        ensure!(
            &crate::device::serial_bytes(&serial)? == expected_serial,
            "BLE firmware serial mismatch"
        );
        let d = self.command(slot, 1, 0x2c, &[0], 0x2e)?;
        ensure!(
            matches!(version(&d)?.as_str(), "4.1.10.0" | "4.1.11.0" | "4.2.0.0"),
            "BLE firmware entry supports supported EP800 4.1.10/4.1.11/4.2.0 only"
        );
        // This is the same stock PC-address retention command used by the wired updater.
        self.drain()?;
        self.send_unit(slot, 1, 0xc0, &[slot])?;
        let reply = self
            .wait(|n| n.characteristic == FREE_RESPONSE && n.value.starts_with(&[slot, 1, 0xc2]))?;
        ensure!(
            reply.value.len() >= 4,
            "truncated address-retention receipt"
        );
        self.drain()?;
        self.firmware_active = true;
        Ok(MotorFirmware {
            session: self,
            slot,
            incoming: VecDeque::new(),
            rx_mode: false,
            dcas_mode: false,
        })
    }
}
impl MotorFirmware<'_> {
    pub fn slot(&self) -> u8 {
        self.slot
    }
    pub fn enter_rx(&mut self) -> Result<crate::rx::Boot> {
        ensure!(
            !self.rx_mode && !self.dcas_mode,
            "motor firmware mode already entered"
        );
        self.session.drain()?;
        self.session.send_unit(self.slot, 0x32, 0x20, &[1])?;
        let reply = self.session.wait(|n| {
            n.characteristic == FREE_RESPONSE && n.value.starts_with(&[self.slot, 0x32, 0x22])
        })?;
        ensure!(
            reply.value.len() >= 4,
            "truncated RX update-mode acknowledgement"
        );
        self.rx_mode = true;
        // PCA_UPDATE_SET is acknowledged before the other MCU has restarted.
        // Let RX enter its bootloader before the first capability request.
        sleep(Duration::from_secs(1));
        crate::rx::read_legacy_boot(self)
    }
    pub fn reset_rx(&mut self) -> Result<()> {
        ensure!(self.rx_mode, "RX mode is not active");
        crate::rx::reset_legacy(self)?;
        self.rx_mode = false;
        Ok(())
    }
    pub fn flash_rx(
        &mut self,
        image: &crate::firmware::Firmware,
        boot: crate::rx::Boot,
        progress: impl FnMut(usize, usize),
    ) -> Result<crate::rx::Receipt> {
        ensure!(
            self.rx_mode && !self.dcas_mode,
            "RX firmware mode is not active"
        );
        let receipt = crate::rx::flash_legacy(self, image, boot, progress)?;
        self.rx_mode = false;
        Ok(receipt)
    }
    pub fn enter_dcas(&mut self) -> Result<BootIdentity> {
        ensure!(
            !self.rx_mode && !self.dcas_mode,
            "motor firmware mode already entered"
        );
        self.mode(0x60)?;
        self.dcas_mode = true;
        self.target(self.slot)?;
        const KEYS: [[u8; 3]; 5] = [
            [0x31, 0x41, 0x59],
            [0x26, 0x53, 0x58],
            [0x97, 0x93, 0x23],
            [0x84, 0x62, 0x64],
            [0x33, 0x83, 0x27],
        ];
        for (i, key) in KEYS.iter().enumerate() {
            let p = [(i + 1) as u8, key[0], key[1], key[2]];
            if i == 4 {
                self.send(0x88, &p)?;
                sleep(Duration::from_secs(1));
                self.target(self.slot)?;
            }
            self.exchange(0x88, &p, Duration::from_secs(5), |f| {
                f.control & 0x88 == 0x88 && f.payload.starts_with(&[0x11, (i + 1) as u8])
            })?;
        }
        firmware_io::boot_identity(self, self.slot)
    }
    pub fn identity(&mut self) -> Result<BootIdentity> {
        ensure!(self.dcas_mode, "DCAS boot mode is not active");
        firmware_io::boot_identity(self, self.slot)
    }
    pub fn reset_dcas(&mut self) -> Result<()> {
        ensure!(self.dcas_mode, "DCAS boot mode is not active");
        firmware_io::boot_reset(self)?;
        self.dcas_mode = false;
        sleep(Duration::from_secs(1));
        Ok(())
    }
    pub fn flash_dcas(
        &mut self,
        image: &[u8],
        identity: &BootIdentity,
        progress: impl FnMut(usize, usize),
    ) -> Result<FlashReceipt> {
        ensure!(
            self.dcas_mode && !self.rx_mode,
            "DCAS firmware mode is not active"
        );
        let receipt = firmware_io::flash_dcas(self, image, identity, progress)?;
        self.dcas_mode = false;
        Ok(receipt)
    }
    pub fn application_return(self) -> Result<()> {
        ensure!(
            !self.dcas_mode && !self.rx_mode,
            "acknowledge motor reset before normal routing"
        );
        ensure!(
            !self.session.poisoned,
            "BLE firmware outcome uncertain; inspect bike before recovery"
        );
        let r = self.session.gateway(3, &[0x20], 0x23)?;
        ensure!(r.first() == Some(&0), "restore normal/master mode rejected");
        sleep(Duration::from_secs(1));
        let r = self.session.gateway(4, &[], 0x24)?;
        ensure!(
            r.first() == Some(&0xa0),
            "normal/master application mode not confirmed"
        );
        self.session.firmware_active = false;
        self.session.linked_slot = None;
        Ok(())
    }
    fn parse(&mut self, n: Notification) -> Result<Option<Frame>> {
        if self.rx_mode
            && n.characteristic == FREE_RESPONSE
            && n.value.first() == Some(&self.slot)
            && n.value.get(1) == Some(&0xf2)
        {
            if !(4..=20).contains(&n.value.len()) {
                self.session.poisoned = true;
                bail!("malformed BLE RX response");
            }
            let mut payload = n.value;
            payload[0] = 0;
            return Ok(Some(Frame {
                control: 0x48,
                payload,
            }));
        }
        if n.characteristic != PCE_RESPONSE {
            return Ok(None);
        }
        if !(2..=20).contains(&n.value.len()) {
            self.session.poisoned = true;
            bail!("malformed BLE firmware response");
        }
        let f = Frame {
            control: n.value[0],
            payload: n.value[1..].to_vec(),
        };
        if (f.control & 0x88 == 0x88 && f.payload.first() == Some(&0x32))
            || (f.control & 0x0b == 0x0b
                && f.payload.len() >= 4
                && matches!(f.payload[1], 0x82 | 0x83 | 0xc0)
                && f.payload[3] != 0)
        {
            self.session.poisoned = true;
            bail!(
                "motor/gateway rejected firmware transfer: {}",
                hex::encode(n.value)
            );
        }
        Ok(Some(f))
    }
    fn drain_firmware(&mut self) -> Result<()> {
        for _ in 0..256 {
            let Some(n) = self.session.receive(Duration::ZERO)? else {
                return Ok(());
            };
            if let Some(f) = self.parse(n)? {
                self.incoming.push_back(f);
            }
            if self.incoming.len() > 256 {
                break;
            }
        }
        self.session.poisoned = true;
        bail!("BLE firmware unsolicited queue overflow")
    }
    fn mode(&mut self, mode: u8) -> Result<()> {
        let r = self.session.gateway(3, &[mode], 0x23)?;
        ensure!(r.first() == Some(&0), "gateway rejected motor update mode");
        sleep(Duration::from_secs(1));
        let r = self.session.gateway(4, &[], 0x24)?;
        ensure!(
            r.first() == Some(&(mode | 0x80)),
            "gateway motor update mode not confirmed"
        );
        Ok(())
    }
    fn target(&mut self, slot: u8) -> Result<()> {
        self.session.select_target(slot)
    }
}
impl FirmwareIo for MotorFirmware<'_> {
    fn exchange(
        &mut self,
        control: u8,
        payload: &[u8],
        timeout: Duration,
        check: impl Fn(&Frame) -> bool,
    ) -> Result<Frame> {
        self.drain_firmware()?;
        self.discard_matching(&check);
        self.send(control, payload)?;
        self.wait(timeout, check)
    }
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        ensure!(
            !self.session.poisoned && self.session.firmware_active,
            "BLE firmware session is fenced"
        );
        if control == 0x48 {
            ensure!(
                self.rx_mode && payload.len() == 7 && matches!(payload[1], 0xf0 | 0xf8),
                "invalid RX bridge request"
            );
            let mut value = payload.to_vec();
            value[0] = self.slot;
            let c = self
                .session
                .chars
                .get(&FREE_COMMAND)
                .context("missing RX relay characteristic")?;
            let streaming = payload[1] == 0xf8;
            // Some stock gateways advertise only acknowledged ATT writes. Keep
            // the same relay records and motor receipts in either ATT mode.
            let without_response = (streaming && c.write_without_response) || !c.write;
            self.session.log("tx", FREE_COMMAND, &value)?;
            if let Err(e) = self
                .session
                .io
                .write(FREE_COMMAND, &value, without_response)
            {
                self.session.poisoned = true;
                return Err(e).context("RX bridge dispatch uncertain");
            }
            return Ok(());
        }
        ensure!(
            matches!(control, 0x88 | 0x0b),
            "unsupported motor firmware relay control"
        );
        let fragments = pce_fragments(control, payload)?;
        let fragmented = fragments.len() > 1;
        let c = self
            .session
            .chars
            .get(&PCE_COMMAND)
            .context("missing motor relay characteristic")?;
        let without_response = (fragmented && c.write_without_response) || !c.write;
        for value in fragments {
            self.session.log("tx", PCE_COMMAND, &value)?;
            if let Err(e) = self.session.io.write(PCE_COMMAND, &value, without_response) {
                self.session.poisoned = true;
                return Err(e).context("motor relay dispatch uncertain; no automatic retry");
            }
        }
        Ok(())
    }
    fn wait(&mut self, timeout: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
        let start = Instant::now();
        loop {
            if let Some(i) = self.incoming.iter().position(&check) {
                return Ok(self.incoming.remove(i).unwrap());
            }
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            let Some(n) = self.session.receive(remaining)? else {
                break;
            };
            let Some(f) = self.parse(n)? else {
                continue;
            };
            if check(&f) {
                return Ok(f);
            }
            self.incoming.push_back(f);
            if self.incoming.len() > 256 {
                break;
            }
        }
        self.session.poisoned = true;
        bail!("BLE firmware response timeout/overflow; no automatic retry")
    }
    fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool) {
        self.incoming.retain(|f| !check(f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stock_large_write_boundary_keeps_empty_terminal_record() {
        for length in [18, 36, 54] {
            let records = pce_fragments(0x0b, &vec![0x55; length]).unwrap();
            assert_eq!(records.len(), length / 18 + 1);
            assert_eq!(
                records.last().unwrap(),
                &vec![(length / 18 * 32) as u8, 0x0b]
            );
            assert!(records[..records.len() - 1]
                .iter()
                .all(|r| r[0] & 0x80 != 0 && r.len() == 20));
        }
    }
    #[test]
    fn stock_ble_fragment_vectors() {
        assert_eq!(
            pce_fragments(0x88, &[0x21, 0, 77, 0]).unwrap(),
            vec![vec![0, 0x88, 0x21, 0, 77, 0]]
        );
        let payload: Vec<u8> = (0..70).collect();
        let records = pce_fragments(0x0b, &payload).unwrap();
        assert_eq!(
            records.iter().map(|r| r[0]).collect::<Vec<_>>(),
            [0x80, 0xa0, 0xc0, 0x60]
        );
        assert_eq!(
            records.iter().map(Vec::len).collect::<Vec<_>>(),
            [20, 20, 20, 18]
        );
        assert!(records.iter().all(|r| r[1] == 0x0b));
        assert_eq!(
            records
                .iter()
                .flat_map(|r| r[2..].iter().copied())
                .collect::<Vec<_>>(),
            payload
        );
        assert!(pce_fragments(0x0b, &[0; 72]).is_err());
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[derive(Default)]
    struct State {
        writes: Vec<(u16, Vec<u8>, bool)>,
        pending: VecDeque<Notification>,
        replies: VecDeque<Vec<Notification>>,
        fail_at: Option<usize>,
    }
    struct Mock(Rc<RefCell<State>>);
    fn n(characteristic: u16, value: &[u8]) -> Notification {
        Notification {
            characteristic,
            value: value.to_vec(),
        }
    }
    impl Gatt for Mock {
        fn characteristics(&self) -> Vec<CharacteristicInfo> {
            [PCE_COMMAND, FREE_COMMAND]
                .into_iter()
                .map(|id| CharacteristicInfo {
                    id,
                    read: false,
                    write: true,
                    write_without_response: true,
                    notify: false,
                    indicate: false,
                })
                .collect()
        }
        fn read(&mut self, _: u16) -> Result<Vec<u8>> {
            bail!("unexpected read")
        }
        fn passkey(&mut self) -> Result<String> {
            bail!("unexpected credentials")
        }
        fn subscribe(&mut self, _: u16) -> Result<()> {
            bail!("unexpected subscription")
        }
        fn write(&mut self, id: u16, value: &[u8], wwr: bool) -> Result<()> {
            let mut s = self.0.borrow_mut();
            s.writes.push((id, value.to_vec(), wwr));
            if s.fail_at == Some(s.writes.len()) {
                bail!("fixture disconnect");
            }
            if let Some(reply) = s.replies.pop_front() {
                s.pending.extend(reply);
            }
            Ok(())
        }
        fn receive(&mut self, _: Duration) -> Result<Option<Notification>> {
            Ok(self.0.borrow_mut().pending.pop_front())
        }
        fn close(&mut self) -> Result<()> {
            Ok(())
        }
    }
    fn session(state: Rc<RefCell<State>>) -> BleSession {
        let mut session = BleSession::new(Box::new(Mock(state)), None).unwrap();
        session.firmware_active = true;
        session
    }
    fn link(session: &mut BleSession, rx_mode: bool) -> MotorFirmware<'_> {
        MotorFirmware {
            session,
            slot: 13,
            incoming: VecDeque::new(),
            rx_mode,
            dcas_mode: !rx_mode,
        }
    }
    #[test]
    fn fragmented_payload_uses_stock_characteristic_headers_and_write_type() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut session = session(state.clone());
        link(&mut session, false).send(0x0b, &[0x55; 70]).unwrap();
        let s = state.borrow();
        assert_eq!(s.writes.len(), 4);
        assert_eq!(
            s.writes.iter().map(|(_, b, _)| b[0]).collect::<Vec<_>>(),
            [0x80, 0xa0, 0xc0, 0x60]
        );
        assert!(s
            .writes
            .iter()
            .all(|(id, b, wwr)| *id == PCE_COMMAND && b[1] == 0x0b && *wwr));
    }
    #[test]
    fn response_only_gateway_preserves_firmware_records_and_order() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut session = session(state.clone());
        for id in [PCE_COMMAND, FREE_COMMAND] {
            session.chars.get_mut(&id).unwrap().write_without_response = false;
        }
        link(&mut session, false).send(0x0b, &[0x55; 70]).unwrap();
        link(&mut session, true)
            .send(0x48, &[0, 0xf8, 0, 1, 2, 3, 4])
            .unwrap();
        let s = state.borrow();
        assert_eq!(s.writes.len(), 5);
        assert!(s.writes.iter().all(|(_, _, without)| !without));
        assert_eq!(
            s.writes[..4]
                .iter()
                .map(|(_, b, _)| b[0])
                .collect::<Vec<_>>(),
            [0x80, 0xa0, 0xc0, 0x60]
        );
        assert_eq!(s.writes[4].0, FREE_COMMAND);
        assert_eq!(s.writes[4].1, [13, 0xf8, 0, 1, 2, 3, 4]);
    }
    #[test]
    fn stale_ack_cannot_verify_finish_and_timeout_fences_dispatch() {
        let state = Rc::new(RefCell::new(State::default()));
        state
            .borrow_mut()
            .pending
            .push_back(n(PCE_RESPONSE, &[0x88, 0x31, 0, 0]));
        let mut session = session(state.clone());
        let mut link = link(&mut session, false);
        assert!(firmware_io::boot_query(&mut link, &[0x27, 9, 0, 0], 0x31, 1, 100).is_err());
        assert!(link.send(0x88, &[0x21, 0, 1, 0]).is_err());
        assert_eq!(state.borrow().writes.len(), 1);
        assert!(session.stop().is_err());
    }
    #[test]
    fn completion_and_motor_ack_survive_arrival_before_transfer_receipt() {
        let state = Rc::new(RefCell::new(State::default()));
        state.borrow_mut().replies.push_back(vec![
            n(PCE_RESPONSE, &[0x88, 0x31, 1, 0]),
            n(PCE_RESPONSE, &[0x0b, 0, 0xc0, 0, 0]),
            n(PCE_RESPONSE, &[0x0b, 0, 0x83, 1, 0]),
        ]);
        let mut session = session(state);
        let mut link = link(&mut session, false);
        link.exchange(0x0b, &[1, 3, 0], Duration::from_secs(1), |f| {
            f.payload.get(1) == Some(&0x83)
        })
        .unwrap();
        link.wait(Duration::from_secs(1), |f| f.payload.get(1) == Some(&0xc0))
            .unwrap();
        link.wait(Duration::from_secs(1), |f| {
            f.control == 0x88 && f.payload == [0x31, 1, 0]
        })
        .unwrap();
        assert!(link.incoming.is_empty());
    }
    #[test]
    fn rejected_fragment_and_nak_stop_without_replay() {
        for fail in [true, false] {
            let state = Rc::new(RefCell::new(State::default()));
            if fail {
                state.borrow_mut().fail_at = Some(2);
            } else {
                state
                    .borrow_mut()
                    .replies
                    .push_back(vec![n(PCE_RESPONSE, &[0x88, 0x32, 5])]);
            }
            let mut session = session(state.clone());
            let mut link = link(&mut session, false);
            let result = if fail {
                link.send(0x0b, &[0; 70])
            } else {
                firmware_io::boot_query(&mut link, &[0x21, 0, 1, 0], 0x31, 1, 100).map(|_| ())
            };
            assert!(result.is_err());
            let count = state.borrow().writes.len();
            assert_eq!(count, if fail { 2 } else { 1 });
            assert!(link.send(0x88, &[0x21, 0, 1, 0]).is_err());
            assert_eq!(state.borrow().writes.len(), count);
        }
    }
    #[test]
    fn rx_capability_and_reset_use_only_selected_motor_free_channel() {
        let state = Rc::new(RefCell::new(State::default()));
        state.borrow_mut().replies.push_back(vec![
            n(FREE_RESPONSE, &[19, 0xf2, 0, 0x51, 0x20, 7, 2]),
            n(FREE_RESPONSE, &[13, 0xf2, 0, 0x51, 0x20, 7, 2]),
        ]);
        state
            .borrow_mut()
            .replies
            .push_back(vec![n(FREE_RESPONSE, &[13, 0xf2, 0, 0x31, 0, 0, 0])]);
        let mut session = session(state.clone());
        let mut link = link(&mut session, true);
        let boot = crate::rx::read_legacy_boot(&mut link).unwrap();
        assert_eq!(boot.version, crate::firmware::Version([2, 0, 7, 2]));
        link.reset_rx().unwrap();
        let s = state.borrow();
        assert_eq!(
            s.writes[0],
            (FREE_COMMAND, vec![13, 0xf0, 0, 0x41, 0, 0, 0], false)
        );
        assert_eq!(
            s.writes[1],
            (FREE_COMMAND, vec![13, 0xf0, 0, 0x28, 0, 0, 0], false)
        );
    }
    #[test]
    fn retry_capable_rx_is_rejected_before_erase() {
        let state = Rc::new(RefCell::new(State::default()));
        state
            .borrow_mut()
            .replies
            .push_back(vec![n(FREE_RESPONSE, &[13, 0xf2, 1, 0x51, 0x20, 7, 2])]);
        let mut session = session(state.clone());
        assert!(crate::rx::read_legacy_boot(&mut link(&mut session, true)).is_err());
        assert_eq!(state.borrow().writes.len(), 1);
    }
}
