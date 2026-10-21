//! E-TUBE BLE sessions. Motor firmware uses a separate guarded relay; radio DFU is absent.
//!
//! Short PCE commands use `[0, control, params..]` on `PCE_COMMAND`; notifications
//! begin with the reply control. FREE requests use `[target, group, opcode,
//! params..]`, and FREE notifications retain the responding component slot.
//! Neither channel uses USB delimiters, FCS or padding. Unlike a USB control-0x48
//! reply, the FREE route byte is the component rather than the PC recipient.
//!
//! Gateway PIN authentication, gateway role/discovery and motor PC-link or
//! protected-setting authorization are separate state transitions. Inventory
//! resolves multiple component types. Explicit component startup requires a
//! unique catalogued identity; default startup and firmware retain motor gates.
//! Master discovery can itself start remote PC links through the gateway;
//! startup consumes those receipts instead of replaying the motor keys.
use crate::{
    device::{Identity, UnitCommands},
    platform::{sleep, Instant},
    storage::{File, OpenOptions},
};
use aes::{
    cipher::{BlockEncrypt, KeyInit},
    Aes128,
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
    time::Duration,
};
use zeroize::Zeroize;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(all(not(target_arch = "wasm32"), feature = "ble"))]
mod native;
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod simulator;

#[cfg(test)]
mod modern_session_tests;
pub(crate) mod unit_auth;
#[cfg(test)]
mod unit_auth_session_tests;

/// Configuration service. Characteristic short IDs below use the same UUID
/// suffix through `uuid`; they are separate channels, not USB control bytes.
pub const SERVICE: &str = "000018ff-5348-494d-414e-4f5f424c4500";
/// Proof writes and authentication events; subscribe before sending proofs.
pub const AUTH_CONTROL: u16 = 0x2af3;
/// Read the gateway nonce; independent of the motor authorization challenge.
pub const AUTH_NONCE: u16 = 0x2af4;
pub const PCE_RESPONSE: u16 = 0x2af9;
pub const PCE_COMMAND: u16 = 0x2afa;
pub const FREE_RESPONSE: u16 = 0x2afd;
pub const FREE_COMMAND: u16 = 0x2afe;
pub const FEATURE: u16 = 0x2aff;
const TIMEOUT: Duration = Duration::from_secs(5);

pub fn uuid(short: u16) -> String {
    format!("0000{short:04x}-5348-494d-414e-4f5f424c4500")
}
pub fn is_port(port: &str) -> bool {
    port == "webble" || port.starts_with("ble:") || port.starts_with("ble-sim:")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CharacteristicInfo {
    pub id: u16,
    pub read: bool,
    pub write: bool,
    pub write_without_response: bool,
    pub notify: bool,
    pub indicate: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Notification {
    pub characteristic: u16,
    pub value: Vec<u8>,
}

/// One exclusive connection. Implementations bound queues, detect disconnects,
/// and terminate the connection on failed/timed-out GATT operations.
pub trait Gatt {
    fn characteristics(&self) -> Vec<CharacteristicInfo>;
    fn read(&mut self, id: u16) -> Result<Vec<u8>>;
    fn write(&mut self, id: u16, bytes: &[u8], without_response: bool) -> Result<()>;
    fn subscribe(&mut self, id: u16) -> Result<()>;
    fn receive(&mut self, timeout: Duration) -> Result<Option<Notification>>;
    fn passkey(&mut self) -> Result<String>;
    fn clear_passkey(&mut self) {}
    fn close(&mut self) -> Result<()>;
}

#[derive(Serialize)]
pub struct PeripheralInfo {
    pub id: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub etube_service_advertised: bool,
}
pub fn scan(seconds: u64, all: bool) -> Result<Vec<PeripheralInfo>> {
    ensure!(
        (1..=30).contains(&seconds),
        "scan duration must be 1..30 seconds"
    );
    #[cfg(all(not(target_arch = "wasm32"), feature = "ble"))]
    {
        native::scan(seconds, all)
    }
    #[cfg(any(target_arch = "wasm32", not(feature = "ble")))]
    {
        let _ = all;
        bail!("Native BLE discovery requires cargo build --features ble; in the browser use connect ble")
    }
}

pub fn open(port: &str, trace: Option<&Path>, passkey: Option<String>) -> Result<BleSession> {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    if let Some(path) = port.strip_prefix("ble-sim:") {
        return BleSession::new(
            Box::new(simulator::SimulatorGatt::open(path)?.with_passkey(passkey)),
            trace,
        );
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    ensure!(
        !port.starts_with("ble-sim:"),
        "BLE simulator currently requires native Unix sockets"
    );
    #[cfg(all(not(target_arch = "wasm32"), feature = "ble"))]
    let io: Box<dyn Gatt> = {
        let id = port
            .strip_prefix("ble:")
            .context("native Bluetooth requires --port ble:DEVICE_ID")?;
        ensure!(!id.is_empty(), "select a device ID from ble scan");
        Box::new(native::NativeGatt::open(id, passkey)?)
    };
    #[cfg(target_arch = "wasm32")]
    let io: Box<dyn Gatt> = {
        ensure!(port == "webble", "browser Bluetooth requires --port webble");
        Box::new(browser::BrowserGatt::open(passkey)?)
    };
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "ble")))]
    {
        let _ = (port, trace, passkey);
        bail!("Bluetooth support is optional; rebuild with cargo build --features ble")
    }
    #[cfg(any(target_arch = "wasm32", feature = "ble"))]
    {
        BleSession::new(io, trace)
    }
}

// Gateway proof: for nonce length n=1..16, fill its first AES-128 ECB block
// with PKCS#7 byte 16-n and retain only n encrypted bytes. For n=16 there is
// no retained extra padding block. This is not the motor challenge scheme.
fn encrypt_challenge(key: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !nonce.is_empty() && nonce.len() <= 16,
        "unsupported BLE challenge length"
    );
    let mut normalized = [0; 16];
    let n = key.len().min(16);
    normalized[..n].copy_from_slice(&key[..n]);
    let mut block = [u8::try_from(16 - nonce.len())?; 16];
    block[..nonce.len()].copy_from_slice(nonce);
    Aes128::new_from_slice(&normalized)?.encrypt_block((&mut block).into());
    normalized.fill(0);
    Ok(block[..nonce.len()].to_vec())
}
// Preserve all six ASCII digits, including leading zeroes. User proof uses
// the ten-byte prefix plus those six bytes; app proof uses a separate key.
fn auth_packets(passkey: &str, nonce: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        passkey.len() == 6 && passkey.bytes().all(|b| b.is_ascii_digit()),
        "BLE passkey must be exactly six ASCII digits"
    );
    const PREFIX: [u8; 10] = [0, 1, 17, 71, 165, 63, 162, 121, 82, 72];
    const APP: [u8; 16] = [
        0, 2, 211, 156, 164, 38, 198, 148, 227, 11, 201, 34, 232, 123, 181, 141,
    ];
    let mut key = PREFIX.to_vec();
    key.extend_from_slice(passkey.as_bytes());
    let mut user = vec![1];
    user.extend(encrypt_challenge(&key, nonce)?);
    key.fill(0);
    let mut app = vec![2];
    app.extend(encrypt_challenge(&APP, nonce)?);
    Ok((user, app))
}

mod firmware;
pub use firmware::MotorFirmware;

pub struct BleSession {
    io: Box<dyn Gatt>,
    chars: BTreeMap<u16, CharacteristicInfo>,
    trace: Option<File>,
    pc_slot: u8,
    target_slot: Option<u8>,
    master_stock: Option<[u8; 2]>,
    linked_slot: Option<u8>,
    gateway_mode: bool,
    authenticated: bool,
    poisoned: bool,
    unit_auth_attempted: bool,
    master_routing: bool,
    master_routing_active: bool,
    gateway_started_slots: BTreeSet<u8>,
    master_pc_start_receipts: BTreeMap<u8, Vec<u8>>,
    firmware_active: bool,
}
impl BleSession {
    pub fn new(io: Box<dyn Gatt>, trace: Option<&Path>) -> Result<Self> {
        let trace = trace
            .map(|p| OpenOptions::new().create_new(true).write(true).open(p))
            .transpose()?;
        let chars = io
            .characteristics()
            .into_iter()
            .map(|c| (c.id, c))
            .collect();
        Ok(Self {
            io,
            chars,
            trace,
            pc_slot: 0,
            target_slot: None,
            master_stock: None,
            linked_slot: None,
            gateway_mode: false,
            authenticated: false,
            poisoned: false,
            unit_auth_attempted: false,
            master_routing: false,
            master_routing_active: false,
            gateway_started_slots: BTreeSet::new(),
            master_pc_start_receipts: BTreeMap::new(),
            firmware_active: false,
        })
    }
    /// Select stock normal/master routing before component discovery.
    /// This changes bus ownership, not nonvolatile settings or firmware.
    pub fn set_master_routing(&mut self, enabled: bool) -> Result<()> {
        ensure!(
            !self.gateway_mode
                && !self.master_routing_active
                && self.linked_slot.is_none()
                && !self.firmware_active,
            "select BLE routing before component discovery"
        );
        self.master_routing = enabled;
        Ok(())
    }
    pub fn is_master_routing(&self) -> bool {
        self.master_routing
    }
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn configure_command_trace(&mut self, path: Option<&Path>) -> Result<()> {
        let trace = path
            .map(|path| OpenOptions::new().create_new(true).write(true).open(path))
            .transpose()?;
        self.trace = trace;
        Ok(())
    }
    /// Release component ownership between browser commands without shutting down
    /// the gateway's Bluetooth radio. Explicit disconnect still calls stop().
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn pause_browser_command(&mut self) -> Result<()> {
        ensure!(
            !self.poisoned && !self.firmware_active,
            "BLE session cannot be retained after failure or during firmware operations"
        );
        self.stop_pc_link()?;
        Ok(())
    }
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn resume_browser_command(&mut self, master: bool) -> Result<()> {
        ensure!(
            self.authenticated
                && !self.poisoned
                && !self.firmware_active
                && self.linked_slot.is_none(),
            "BLE session is not ready for another command"
        );
        ensure!(
            !self.master_routing_active || master,
            "disconnect before switching an active browser master session to normal routing"
        );
        self.master_routing = master;
        self.unit_auth_attempted = false;
        self.target_slot = None;
        self.master_stock = None;
        Ok(())
    }
    fn log(&mut self, direction: &str, id: u16, bytes: &[u8]) -> Result<()> {
        // Authentication material never enters traces, including nonce/status.
        if matches!(id, AUTH_CONTROL | AUTH_NONCE) {
            return Ok(());
        }
        // The optional drive-unit exchange contains derived credentials and a
        // challenge/proof, distinct from the gateway's GATT authentication.
        if matches!(id, FREE_COMMAND | FREE_RESPONSE)
            && bytes.len() >= 3
            && ((bytes[1] == 0x16 && matches!(bytes[2], 0xd8 | 0xda | 0xdb | 0xe0 | 0xe2 | 0xe3))
                || (bytes[1] == 0x33
                    && matches!(bytes[2], 0x68 | 0x6a | 0x6b | 0x70 | 0x72 | 0x73)))
        {
            return Ok(());
        }
        if let Some(f) = &mut self.trace {
            writeln!(
                f,
                "{}",
                serde_json::json!({"time_ms":crate::platform::now_ms(),"transport":"ble","direction":direction,"characteristic":uuid(id),"bytes":hex::encode(bytes)})
            )?;
            f.flush()?;
        }
        Ok(())
    }
    fn write(&mut self, id: u16, bytes: &[u8]) -> Result<()> {
        ensure!(
            !self.poisoned,
            "BLE session failed; disconnect and reconnect before another command"
        );
        ensure!(
            matches!(id, AUTH_CONTROL | FEATURE | PCE_COMMAND | FREE_COMMAND),
            "BLE firmware/boot operations are disabled"
        );
        ensure!(
            id != FEATURE || bytes == [0xff, 0],
            "BLE mode switching/DFU is disabled"
        );
        ensure!(bytes.len() <= 20, "fragmented BLE transfers are disabled");
        let c = self
            .chars
            .get(&id)
            .context("required BLE characteristic is absent")?;
        ensure!(
            c.write || c.write_without_response,
            "BLE characteristic is not writable"
        );
        let without_response = !c.write;
        self.log("tx", id, bytes)?;
        let result = self.io.write(id, bytes, without_response);
        if result.is_err() {
            self.poisoned = true;
        }
        result.context("BLE write failed; outcome may be uncertain")
    }
    fn receive(&mut self, timeout: Duration) -> Result<Option<Notification>> {
        let result = self.io.receive(timeout);
        if result.is_err() {
            self.poisoned = true;
        }
        let n = result?;
        if let Some(n) = &n {
            self.log("rx", n.characteristic, &n.value)?;
            if n.characteristic == FREE_RESPONSE
                && (4..=20).contains(&n.value.len())
                && self.gateway_started_slots.contains(&n.value[0])
                && n.value[1..4] == [0x32, 0x12, 1]
            {
                self.master_pc_start_receipts
                    .insert(n.value[0], n.value.clone());
            }
        }
        Ok(n)
    }
    fn drain(&mut self) -> Result<()> {
        // Drop stale replies before dispatch, with a hard bound on unsolicited traffic.
        for _ in 0..256 {
            if self.receive(Duration::ZERO)?.is_none() {
                return Ok(());
            }
        }
        self.poisoned = true;
        bail!("BLE unsolicited notification overflow")
    }
    fn wait(&mut self, check: impl Fn(&Notification) -> bool) -> Result<Notification> {
        let start = Instant::now();
        for _ in 0..256 {
            let remaining = TIMEOUT.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            if let Some(n) = self.receive(remaining)? {
                if n.value.len() > 20 {
                    self.poisoned = true;
                    bail!("unsupported oversized BLE notification");
                }
                if check(&n) {
                    return Ok(n);
                }
            } else {
                break;
            }
        }
        self.poisoned = true;
        bail!("BLE response timeout or notification overflow; reconnect before another command")
    }
    pub fn authenticate(&mut self) -> Result<()> {
        let result = self.authenticate_gateway();
        if result.is_err() {
            self.io.clear_passkey();
        }
        result
    }
    /// Both auth characteristics must exist, or both must be absent. When
    /// present, read nonce (one 100 ms retry if empty), subscribe to auth events,
    /// then write 01|user-proof and 02|app-proof with response. Next perform
    /// Feature FF/00 access check, subscribe to PCE/FREE replies, and select
    /// normal/slave mode. Only PCE 04 readback with high bits 80 confirms this
    /// setup; OS pairing or successful proof writes alone cannot establish it.
    fn authenticate_gateway(&mut self) -> Result<()> {
        ensure!(!self.authenticated, "BLE session already authenticated");
        for id in [PCE_COMMAND, FREE_COMMAND, FEATURE] {
            let c = self
                .chars
                .get(&id)
                .context("device lacks the E-TUBE configuration characteristics")?;
            ensure!(
                c.write || c.write_without_response,
                "required E-TUBE characteristic is not writable"
            );
        }
        let auth = self.chars.contains_key(&AUTH_CONTROL);
        ensure!(
            auth == self.chars.contains_key(&AUTH_NONCE),
            "incomplete BLE authentication service"
        );
        if auth {
            ensure!(
                self.chars[&AUTH_CONTROL].write,
                "BLE authentication requires writes with response"
            );
            let mut nonce = self.io.read(AUTH_NONCE)?;
            if nonce.is_empty() {
                sleep(Duration::from_millis(100));
                nonce = self.io.read(AUTH_NONCE)?;
            }
            ensure!(
                !nonce.is_empty() && nonce.len() <= 16,
                "unsupported BLE challenge length"
            );
            self.io.subscribe(AUTH_CONTROL)?;
            let mut passkey = self.io.passkey()?;
            let packets = auth_packets(&passkey, &nonce);
            // Best-effort clearing; credentials are never logged or serialized to disk.
            passkey.zeroize();
            nonce.fill(0);
            let (mut user, mut app) = packets?;
            let result = self
                .write(AUTH_CONTROL, &user)
                .and_then(|_| self.write(AUTH_CONTROL, &app));
            user.fill(0);
            app.fill(0);
            result.context("BLE authentication failed; check the passkey and OS pairing")?;
        }
        // Vendor access check, not reset/DFU (which uses 00 80 on FEATURE).
        self.write(FEATURE, &[0xff, 0])?;
        for id in [PCE_RESPONSE, FREE_RESPONSE] {
            let c = self
                .chars
                .get(&id)
                .context("missing E-TUBE response characteristic")?;
            ensure!(
                c.notify || c.indicate,
                "required E-TUBE response characteristic cannot notify/indicate"
            );
            self.io.subscribe(id)?;
        }
        // Select normal/slave mode before checking the resulting state.
        // A newly authenticated gateway need not start in that protocol state.
        self.gateway(3, &[0], 0x23)?;
        sleep(Duration::from_secs(1));
        // A successful GATT write is insufficient: require a protocol reply.
        let mode = self.gateway(4, &[], 0x24)?;
        ensure!(
            !mode.is_empty() && mode[0] & 0xe0 == 0x80,
            "invalid BLE gateway protocol identity"
        );
        self.pc_slot = mode[0] & 31;
        self.authenticated = true;
        Ok(())
    }
    fn gateway(&mut self, control: u8, params: &[u8], reply: u8) -> Result<Vec<u8>> {
        ensure!(
            matches!(control, 3 | 4 | 5 | 6 | 0x0c | 0x0d | 0x13 | 0x1b | 0x1d),
            "BLE gateway operation disabled"
        );
        ensure!(
            control != 0x13 || params == [1, 0x1c, 0],
            "only the gateway stock GET is enabled on the local command channel"
        );
        ensure!(
            params.len() <= 16,
            "fragmented BLE gateway command disabled"
        );
        if control == 3 {
            self.target_slot = None;
            self.master_stock = None;
            self.gateway_started_slots.clear();
            self.master_pc_start_receipts.clear();
        }
        self.drain()?;
        let mut bytes = vec![0, control];
        bytes.extend_from_slice(params);
        self.write(PCE_COMMAND, &bytes)?;
        let n =
            self.wait(|n| n.characteristic == PCE_RESPONSE && n.value.first() == Some(&reply))?;
        Ok(n.value[1..].to_vec())
    }
    /// Slave discovery enters gateway PC-link (0C/01), reads master information
    /// (0D/00), then uses routed 01/14 rather than the USB master's PCE 1B.
    /// The bitmap can omit the master; verify its identity separately at slot 0.
    pub fn discover_units(&mut self) -> Result<Vec<u8>> {
        ensure!(
            !self.firmware_active,
            "motor firmware session remains active"
        );
        ensure!(self.authenticated, "authenticate the BLE gateway first");
        if self.master_routing {
            return self.discover_master_units();
        }
        self.gateway(3, &[0], 0x23)?;
        sleep(Duration::from_secs(1));
        let mode = self.gateway(4, &[], 0x24)?;
        ensure!(
            !mode.is_empty() && mode[0] & 0xe0 == 0x80,
            "BLE gateway did not enter normal slave mode"
        );
        self.pc_slot = mode[0] & 31;
        // Enters the gateway PC connection mode; always attempt to leave on stop.
        self.gateway_mode = true;
        // Require the matching 2C receipt; this reply has no USB-style status byte.
        self.gateway(0x0c, &[1], 0x2c)?;
        // Query the bus master separately as logical slot zero. The slave bitmap can
        // omit this master even when it is the motor we need to read.
        let master = self.gateway(0x0d, &[0], 0x2d)?;
        ensure!(master.len() >= 3, "truncated BLE master identity");
        // This handler's battery-present status is not evidence of a complete
        // traction BMS handshake or assist readiness.
        ensure!(master[0] == 1, "BLE master reports no connected battery");
        if master[1] != 0xff {
            // GET_UNIT_INFO byte 2 is a compatibility flag, NOT the unit number.
            // Read the master stock identity to resolve its unit number.
            let stock = self.command(0, 1, 0x1c, &[0], 0x1e)?;
            ensure!(stock.len() >= 2, "truncated BLE master stock identity");
            ensure!(
                stock[0] == master[1],
                "BLE master stock series differs from gateway identity"
            );
            self.master_stock = Some([stock[0], stock[1]]);
        }
        // Read connected slots through the gateway's routed unit query,
        // not SM-PCE02's PCE 1B bitmap (which is empty on SC-EM800).
        let p = self.command(0, 1, 0x14, &[0], 0x16)?;
        ensure!(p.len() >= 6, "invalid BLE connected-unit bitmap length");
        let mut slots: Vec<u8> = (0..32)
            .filter(|&s| s != self.pc_slot && p[2 + usize::from(s) / 8] & (1 << (s % 8)) != 0)
            .collect();
        if self.master_stock.is_some() && !slots.contains(&0) {
            slots.insert(0, 0);
        }
        Ok(slots)
    }
    /// Establish the gateway's identity, select master 20 -> A0 (PC source 0),
    /// then PnP 1D/00, wait 5 s, read 1B and start gateway PC-link 0C/01.
    /// That last step sends the remote motor start and keys on our behalf;
    /// `start_component` must consume its receipt and verify owner zero.
    /// Role changes invalidate cached addresses and target selection.
    fn discover_master_units(&mut self) -> Result<Vec<u8>> {
        if self.master_routing_active {
            let mode = self.gateway(4, &[], 0x24)?;
            ensure!(
                mode.first() == Some(&0xa0),
                "retained BLE master routing was lost; reconnect before another command"
            );
            let bitmap = self.gateway(0x1b, &[], 0x3b)?;
            ensure!(bitmap.len() >= 4, "truncated BLE master component bitmap");
            let slots: Vec<u8> = (1..31)
                .filter(|&s| bitmap[usize::from(s) / 8] & (1 << (s % 8)) != 0)
                .collect();
            ensure!(
                !slots.is_empty(),
                "retained BLE master session has no remote components"
            );
            return Ok(slots);
        }
        // Master routing must establish PC owner zero before privileged commands.
        // Admit gateway revisions by identity and acknowledged state transitions;
        // require the motor to confirm the resulting owner before authentication.
        let revision = self.gateway(5, &[], 0x25)?;
        ensure!(
            revision.len() >= 3,
            "truncated BLE gateway firmware revision"
        );
        let stock = self.gateway(0x13, &[1, 0x1c, 0], 0x33)?;
        ensure!(
            stock.len() >= 4
                && stock.starts_with(&[1, 0x1e])
                && !matches!(stock[2], 0 | 0xff)
                && stock[3] != 0xff,
            "truncated or invalid BLE gateway stock identity"
        );
        self.master_routing_active = true; // Also restore after a partial setup.
        let reply = self.gateway(3, &[0x20], 0x23)?;
        ensure!(
            reply.first() == Some(&0),
            "BLE normal/master routing rejected"
        );
        sleep(Duration::from_secs(1));
        let mode = self.gateway(4, &[], 0x24)?;
        ensure!(
            mode.first() == Some(&0xa0),
            "BLE gateway did not confirm normal/master source zero"
        );
        self.pc_slot = 0;
        let reply = self.gateway(0x1d, &[0], 0x3d)?;
        ensure!(reply.starts_with(&[0, 0]), "BLE master PnP rejected");
        sleep(Duration::from_secs(5));
        let bitmap = self.gateway(0x1b, &[], 0x3b)?;
        ensure!(bitmap.len() >= 4, "truncated BLE master component bitmap");
        let slots: Vec<u8> = (1..31)
            .filter(|&s| bitmap[usize::from(s) / 8] & (1 << (s % 8)) != 0)
            .collect();
        ensure!(
            !slots.is_empty(),
            "BLE master PnP found no remote components"
        );
        self.gateway_mode = true;
        self.gateway_started_slots = slots.iter().copied().collect();
        self.master_pc_start_receipts.clear();
        let reply = self.gateway(0x0c, &[1], 0x2c)?;
        ensure!(
            reply.first() == Some(&0),
            "BLE master PC-link start rejected"
        );
        Ok(slots)
    }
    fn select_target(&mut self, slot: u8) -> Result<()> {
        // Select the bus target before direct
        // FREE_COMMAND traffic, including logical master/local target zero.
        // Never assume a fresh gateway has acknowledged the desired target.
        if self.target_slot != Some(slot) {
            let reply = self.gateway(6, &[slot], 0x26)?;
            ensure!(
                reply.first() == Some(&0),
                "BLE target selection rejected for slot {slot}"
            );
            self.target_slot = Some(slot);
        }
        Ok(())
    }
    fn send_unit(&mut self, slot: u8, group: u8, command: u8, params: &[u8]) -> Result<()> {
        let model_query = slot == 0
            && group == 2
            && command == 0x14
            && params.len() == 1
            && (1..32).contains(&params[0])
            && params[0] != self.pc_slot;
        ensure!(
            slot < 32 && (slot != self.pc_slot || model_query),
            "invalid BLE target slot"
        );
        ensure!(
            params.len() <= 15,
            "fragmented BLE settings commands are disabled"
        );
        // Normal/master FREE_COMMAND carries the remote target directly into
        // firmware 29280 -> 2BD20. PCE6 additionally rejects the gateway's old
        // slave address, which can now belong to a newly enumerated component.
        // The ordinary slave/master-zero route still requires explicit PCE6.
        if !self.master_routing {
            self.select_target(slot)?;
        }
        let mut bytes = vec![slot, group, command];
        bytes.extend_from_slice(params);
        self.write(FREE_COMMAND, &bytes)
    }
    fn inventory_slots(
        &mut self,
        slots: &[u8],
        resolve_names: bool,
    ) -> Result<Vec<crate::device::UnitIdentity>> {
        let mut units = Vec::new();
        for &slot in slots {
            let mut unit =
                crate::device::UnitIdentity::from_reply(slot, self.model_identity(slot)?)?;
            if resolve_names {
                unit.resolve_name(self)?;
            }
            units.push(unit);
        }
        Ok(units)
    }
    pub fn units(&mut self) -> Result<Vec<crate::device::UnitIdentity>> {
        let slots = self.discover_units()?;
        self.inventory_slots(&slots, true)
    }
    pub fn start(&mut self, selected: Option<u8>) -> Result<Identity> {
        self.start_inner(selected, None, false)
    }
    pub fn start_component(
        &mut self,
        selected: Option<u8>,
        component: Option<&str>,
    ) -> Result<Identity> {
        self.start_inner(
            selected,
            component,
            selected.is_some() || component.is_some(),
        )
    }
    fn start_inner(
        &mut self,
        selected: Option<u8>,
        component: Option<&str>,
        allow_component: bool,
    ) -> Result<Identity> {
        let slots = self.discover_units()?;
        let slot = if let Some(component) = component {
            let named_slots = match selected {
                Some(slot) => vec![crate::device::select_motor_slot(&slots, Some(slot))?],
                None => slots.clone(),
            };
            let units = self.inventory_slots(&named_slots, true)?;
            crate::device::select_named_component(&units, component)?
        } else if selected.is_none() && slots.len() > 1 {
            crate::device::select_current_motor(&self.inventory_slots(&slots, false)?)?
        } else {
            crate::device::select_motor_slot(&slots, selected)?
        };
        let mut unit = crate::device::UnitIdentity::from_reply(slot, self.model_identity(slot)?)?;
        unit.prepare_session(self, allow_component)?;
        if let Some(component) = component {
            if unit.model.is_none() {
                unit.resolve_name(self)?;
            }
            ensure!(
                unit.model
                    .is_some_and(|model| crate::unit_models::name_matches(component, model)),
                "component {component:?} no longer matches slot {slot}"
            );
        }
        self.linked_slot = Some(slot);
        // Initial master setup broadcasts PC start and all five keys through
        // the gateway. Preserve its component receipt instead of starting twice.
        // Later commands restart only the component released by the previous pause.
        let receipt = if self.master_routing && self.gateway_started_slots.contains(&slot) {
            Some(
                if let Some(value) = self.master_pc_start_receipts.get(&slot).cloned() {
                    Notification {
                        characteristic: FREE_RESPONSE,
                        value,
                    }
                } else {
                    self.wait(|n| {
                        n.characteristic == FREE_RESPONSE
                            && n.value.starts_with(&[slot, 0x32, 0x12, 1])
                    })?
                },
            )
        } else if slot != 0 {
            self.drain()?;
            // Same logical PC-link start and unit keys as the wired application,
            // routed over FREE_COMMAND rather than serial/USB framing.
            self.send_unit(slot, 0x32, 0x10, &[1, self.pc_slot, 0, 0])?;
            sleep(Duration::from_secs(1));
            for key in [
                [0xa2, 0x2b],
                [0x30, 0x0e],
                [0x7a, 0x4d],
                [0x62, 0x2b],
                [0x85, 0xb4],
            ] {
                self.send_unit(slot, 0x32, 0x30, &[key[0], key[1], 0, 0])?;
                sleep(Duration::from_millis(100));
            }
            let pc = slot;
            Some(self.wait(|n| {
                n.characteristic == FREE_RESPONSE && n.value.starts_with(&[pc, 0x32, 0x12, 1])
            })?)
        } else {
            None
        };
        if self.master_routing {
            if let Some(receipt) = receipt {
                ensure!(
                    receipt.value.len() >= 7
                        && receipt.value[4] == 0
                        && receipt.value[5..7] == [0xff, 0xff],
                    "BLE component did not confirm PC owner zero; privileged routing is unverified"
                );
            }
        }
        self.gateway_started_slots.remove(&slot);
        self.master_pc_start_receipts.remove(&slot);
        if !unit.is_motor() {
            return Ok(Identity {
                slot,
                cargo: unit.model.context("unresolved component model")?.to_owned(),
                dcas_version: version(&self.command(slot, 1, 0x2c, &[0], 0x2e)?)?,
                rx_version: "not applicable".to_owned(),
            });
        }
        let cargo = crate::motor_support::read_params(self, slot, unit.model_series)?;
        ensure!(
            unit.model_series == 0x08 || cargo.len() >= 7,
            "truncated BLE motor attribute reply"
        );
        let d = self.command(slot, 1, 0x2c, &[0], 0x2e)?;
        let rx_version = version(&self.command(slot, 1, 0x84, &[1], 0x86)?)?;
        Ok(Identity {
            slot,
            cargo: String::from_utf8_lossy(&cargo)
                .trim_end_matches('\0')
                .into(),
            dcas_version: version(&d)?,
            rx_version,
        })
    }
    /// Release motor ownership, restore/confirm slave mode, then stop the
    /// gateway PC-link before closing GATT. A timeout fences further traffic;
    /// after that, disconnect cannot stand in for an acknowledged motor stop.
    pub fn stop(&mut self) -> Result<()> {
        ensure!(
            !self.firmware_active,
            "motor firmware session remains active; inspect journal before recovery"
        );
        if self.poisoned {
            bail!("BLE session failed; PC-link stop could not be confirmed; disconnect and inspect bike state");
        }
        let was_master = self.master_routing_active;
        // Release the selected motor while it still acknowledges owner zero.
        // The gateway's local PC-link stop is last because it also shuts down
        // a connected Bluetooth session; it cannot replace the motor's stop.
        let stopped = self.stop_pc_link();
        // Gateway PC-link stop schedules Bluetooth shutdown on a connected
        // session. Restore and confirm slave routing before that final command.
        // Timeouts still fence all further protocol traffic.
        if !was_master {
            stopped?;
            return self.stop_gateway_pc_link();
        }
        let mut errors = Vec::new();
        if let Err(error) = stopped {
            errors.push(format!("unit stop: {error:#}"));
        }
        if !self.poisoned {
            if let Err(error) = self.restore_slave_routing() {
                errors.push(format!("slave routing restoration: {error:#}"));
            }
        }
        if !self.poisoned {
            if let Err(error) = self.stop_gateway_pc_link() {
                errors.push(format!("gateway stop: {error:#}"));
            }
        }
        if !errors.is_empty() {
            let state = if self.master_routing_active {
                "gateway slave routing was not confirmed"
            } else {
                "gateway slave routing was restored"
            };
            bail!("BLE cleanup failed; {state}: {}", errors.join("; "));
        }
        Ok(())
    }
    fn stop_pc_link(&mut self) -> Result<()> {
        if let Some(slot) = self.linked_slot {
            if slot != 0 {
                let p = self.command(slot, 0x32, 0x10, &[0, self.pc_slot, 0, 0], 0x12)?;
                ensure!(p.first() == Some(&0), "BLE unit PC-link stop rejected");
            }
            self.gateway_started_slots.remove(&slot);
            self.master_pc_start_receipts.remove(&slot);
            // The master is stopped by the matching gateway 0C/00 below.
        }
        self.linked_slot = None;
        Ok(())
    }
    fn stop_gateway_pc_link(&mut self) -> Result<()> {
        if self.gateway_mode {
            let reply = self.gateway(0x0c, &[0], 0x2c)?;
            ensure!(
                reply.first() == Some(&0),
                "BLE gateway PC-link stop rejected"
            );
            self.gateway_mode = false;
            self.linked_slot = None;
        }
        Ok(())
    }
    fn restore_slave_routing(&mut self) -> Result<()> {
        let reply = self.gateway(3, &[0], 0x23)?;
        ensure!(
            matches!(reply.first(), Some(0 | 0x10)),
            "BLE normal/slave restoration rejected"
        );
        // A master-to-slave transition can complete asynchronously with 23/10
        // without an immediate 23/00. Neither
        // receipt proves the resulting mode; the following PCE4 must confirm it.
        sleep(Duration::from_secs(1));
        let mode = self.gateway(4, &[], 0x24)?;
        ensure!(
            mode.first().is_some_and(|m| m & 0xe0 == 0x80),
            "BLE normal/slave restoration was not confirmed"
        );
        self.pc_slot = mode[0] & 31;
        self.master_routing_active = false;
        self.linked_slot = None;
        Ok(())
    }
    /// Close GATT only; use `stop` first to confirm protocol-level cleanup.
    pub fn close(&mut self) -> Result<()> {
        self.io.close()
    }

    /// One attempt using the authorization protocol for the live motor family.
    /// This neither changes settings nor enters an update/boot mode.
    pub fn authenticate_unit_settings(
        &mut self,
        expected_serial: &str,
    ) -> Result<serde_json::Value> {
        ensure!(
            !self.poisoned && self.authenticated && self.gateway_mode,
            "normal BLE session required before drive-unit authentication"
        );
        ensure!(
            !self.unit_auth_attempted,
            "drive-unit authentication already attempted; no automatic retry"
        );
        let slot = self
            .linked_slot
            .context("start the motor PC-link session first")?;
        let serial = self.command(slot, 1, 0x3c, &[0], 0x3e)?;
        let serial = crate::device::serial_bytes(&serial)?;
        ensure!(
            hex::encode(serial).eq_ignore_ascii_case(expected_serial),
            "drive-unit authentication motor serial mismatch"
        );
        let model = self.model_identity(slot)?;
        ensure!(model.len() >= 3, "invalid motor stock identity");
        crate::device::UnitIdentity::from_reply(slot, model.clone())?.require_motor()?;
        let cargo = crate::motor_support::read_params(self, slot, model[1])?;
        let d = self.command(slot, 1, 0x2c, &[0], 0x2e)?;
        ensure!(
            model.len() >= 3 && model[2] == 0,
            "unsupported motor identity"
        );
        let firmware = version(&d)?;
        let protocol =
            crate::motor_support::authorization_for_motor(model[1], model[2], &cargo, &firmware)?;
        if protocol == crate::motor_support::AuthProtocol::NotRequired {
            self.unit_auth_attempted = true;
            return Ok(
                serde_json::json!({"unit_authentication_required":false,"firmware":firmware,
                "scope":"this motor firmware does not require regulation-setting authentication"}),
            );
        }
        let modern = protocol == crate::motor_support::AuthProtocol::Modern;
        let product = if modern {
            let first = self.command(slot, 1, 0xa4, &[0], 0xa6)?;
            let last = self.command(slot, 1, 0xac, &[0], 0xae)?;
            ensure!(
                first.len() >= 6 && last.len() >= 6,
                "invalid product serial reply"
            );
            let bytes = [&first[1..6], &last[..6]].concat();
            ensure!(
                bytes.iter().all(u8::is_ascii_graphic),
                "invalid product serial characters"
            );
            Some(String::from_utf8(bytes)?)
        } else {
            None
        };
        self.unit_auth_attempted = true;
        if let Some(product) = product {
            let result = self.authenticate_modern(slot, &serial, &product, model[1], &firmware);
            if result.is_err() {
                self.poisoned = true;
            }
            return result;
        }
        let result = (|| -> Result<serde_json::Value> {
            let serial_bytes = serial;
            let keys = unit_auth::derive(&serial_bytes);
            self.drain()?;
            self.send_unit(slot, 0x16, 0xd8, &keys.auth_key)?;
            let mut challenge = zeroize::Zeroizing::new([0u8; 16]);
            for (index, (selector, offset, length)) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)]
                .into_iter()
                .enumerate()
            {
                let pc = slot;
                let n = self.wait(|n| {
                    n.characteristic == FREE_RESPONSE
                        && n.value.len() >= 3
                        && n.value[0] == pc
                        && n.value[1] == 0x16
                        && matches!(n.value[2], 0xda | 0xdb)
                })?;
                ensure!(
                    n.value[2] == 0xda,
                    "drive-unit authentication challenge rejected; no retry"
                );
                ensure!(
                    n.value.len() == 10 && n.value[3] == selector,
                    "invalid/out-of-order drive-unit challenge fragment {}",
                    index + 1
                );
                challenge[offset..offset + length].copy_from_slice(&n.value[4..4 + length]);
            }
            let proof = zeroize::Zeroizing::new(unit_auth::encrypt_challenge(&keys, &challenge));
            for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
                let mut params = zeroize::Zeroizing::new([0xffu8; 7]);
                params[0] = selector;
                params[1..1 + length].copy_from_slice(&proof[offset..offset + length]);
                self.send_unit(slot, 0x16, 0xe0, params.as_ref())?;
                sleep(Duration::from_millis(100));
            }
            let pc = slot;
            let n = self.wait(|n| {
                n.characteristic == FREE_RESPONSE
                    && n.value.len() >= 3
                    && n.value[0] == pc
                    && n.value[1] == 0x16
                    && matches!(n.value[2], 0xe2 | 0xe3)
            })?;
            unit_auth::validate_ble_receipt(&n.value, pc, 0x16, 0xe2)?;
            Ok(serde_json::json!({"unit_authentication_receipt":true,
                "reply_parameters_hex":hex::encode(&n.value[3..7]),"internal_status":0,"hardware_validated":false,
                "firmware":firmware,"protocol":"legacy","scope":"regulation-setting authentication; per-setting validation still required"}))
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn auth_fragments(&mut self, slot: u8, command: u8, bytes: &[u8; 16]) -> Result<()> {
        for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
            let mut params = zeroize::Zeroizing::new([0xff; 7]);
            params[0] = selector;
            params[1..1 + length].copy_from_slice(&bytes[offset..offset + length]);
            self.send_unit(slot, 0x33, command, params.as_ref())?;
            sleep(Duration::from_millis(100));
        }
        Ok(())
    }
    fn authenticate_modern(
        &mut self,
        slot: u8,
        serial: &[u8; 6],
        product: &str,
        series: u8,
        firmware: &str,
    ) -> Result<serde_json::Value> {
        let keys = unit_auth::derive_modern(serial, product)?;
        self.drain()?;
        let ciphertext = zeroize::Zeroizing::new(keys.auth_ciphertext());
        self.auth_fragments(slot, 0x68, &ciphertext)?;
        let mut challenge = zeroize::Zeroizing::new([0u8; 16]);
        for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
            let pc = slot;
            let n = self.wait(|n| {
                n.characteristic == FREE_RESPONSE
                    && n.value.len() >= 3
                    && n.value[0] == pc
                    && n.value[1] == 0x33
                    && matches!(n.value[2], 0x6a | 0x6b)
            })?;
            ensure!(
                n.value[2] == 0x6a && n.value.len() == 10 && n.value[3] == selector,
                "modern authentication challenge rejected or malformed; no retry"
            );
            challenge[offset..offset + length].copy_from_slice(&n.value[4..4 + length]);
        }
        let proof = zeroize::Zeroizing::new(keys.challenge_proof(&challenge));
        self.auth_fragments(slot, 0x70, &proof)?;
        let pc = slot;
        let n = self.wait(|n| {
            n.characteristic == FREE_RESPONSE
                && n.value.len() >= 3
                && n.value[0] == pc
                && n.value[1] == 0x33
                && matches!(n.value[2], 0x72 | 0x73)
        })?;
        unit_auth::validate_ble_receipt(&n.value, pc, 0x33, 0x72)?;
        Ok(
            serde_json::json!({"unit_authentication_receipt":true,"reply_parameters_hex":hex::encode(&n.value[3..7]),"internal_status":0,
            "hardware_validated":false,"protocol":"modern","series":series,"firmware":firmware,
            "scope":"regulation-setting authentication; per-setting validation still required"}),
        )
    }
}
impl UnitCommands for BleSession {
    fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
        if self.master_routing {
            ensure!(
                (1..31).contains(&slot),
                "BLE master component must have a remote slot"
            );
            let stock = self.command(slot, 1, 0x1c, &[0], 0x1e)?;
            ensure!(stock.len() >= 2, "truncated BLE component stock identity");
            return Ok(vec![slot, stock[0], stock[1]]);
        }
        if slot == 0 {
            ensure!(
                self.authenticated && self.gateway_mode,
                "BLE settings session is not active"
            );
            let stock = self
                .master_stock
                .context("BLE master identity is unavailable; discover components first")?;
            return Ok(vec![0, stock[0], stock[1]]);
        }
        // Non-master stock identities come from the gateway's session cache.
        self.command(0, 2, 0x14, &[slot], 0x16)
    }
    fn command(
        &mut self,
        slot: u8,
        group: u8,
        command: u8,
        params: &[u8],
        response_cmd: u8,
    ) -> Result<Vec<u8>> {
        ensure!(
            self.authenticated && self.gateway_mode && !self.firmware_active,
            "BLE settings session is not active"
        );
        // Only settings schema operations and identity/PC-link commands reach GATT.
        ensure!(
            allowed_unit_command(group, command),
            "BLE firmware/boot or unsupported unit command is disabled"
        );
        self.drain()?;
        self.send_unit(slot, group, command, params)?;
        // FREE_RESPONSE carries the addressed component slot, independently
        // of the gateway's bus PC-link slot. Local gateway queries use slot 0.
        let pc = slot;
        let n = self.wait(|n| {
            n.characteristic == FREE_RESPONSE
                && n.value.len() >= 3
                && n.value[0] == pc
                && n.value[1] == group
                && (n.value[2] == response_cmd || n.value[2] == response_cmd.wrapping_add(1))
        })?;
        ensure!(
            n.value[2] == response_cmd,
            "unit/gateway rejected BLE group {group:02X} command {command:02X}: reply {}",
            hex::encode(&n.value)
        );
        Ok(n.value[3..].to_vec())
    }
}
fn allowed_unit_command(group: u8, command: u8) -> bool {
    matches!(
        (group, command),
        (1, 0x14 | 0x1c | 0x2c | 0x84 | 0x3c | 0xa4 | 0xac)
            | (2, 0x14)
            | (4, 0x0c)
            | (0x16, 0x7c)
            | (0x32, 0x10 | 0x9c)
    ) || crate::settings::supports_command(group, command)
}
fn version(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() >= 3, "short BLE MCU version reply");
    Ok(format!(
        "{}.{}.{}.{}",
        bytes[0] >> 4,
        bytes[0] & 15,
        bytes[1],
        bytes[2]
    ))
}
impl Drop for BleSession {
    fn drop(&mut self) {
        let _ = self.io.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};
    #[derive(Default)]
    struct State {
        writes: Vec<(u16, Vec<u8>)>,
        pending: VecDeque<Notification>,
        subscriptions: Vec<u16>,
        lamp: u8,
        destination: u8,
        speed_limit: u16,
        unit_authenticated: bool,
        reject_destination_write: bool,
        reject_lamp: bool,
        timeout_lamp: bool,
        invalid_nonce: bool,
        bad_mode: bool,
        unit_bitmap: Option<[u8; 4]>,
        stock_series: Option<u8>,
        stock_unit: Option<u8>,
        component_part: Option<u8>,
        component_stock: BTreeMap<u8, [u8; 2]>,
        stock_sequence: VecDeque<[u8; 2]>,
        master_info: Option<Vec<u8>>,
        master_stock_reply: Option<Vec<u8>>,
        motor_params: Option<Vec<u8>>,
        motor_version: Option<Vec<u8>>,
        initial_extended_mode: bool,
        mode_negotiated: bool,
        master_mode: bool,
        reject_master_mode: bool,
        wrong_pc_owner: bool,
        omit_gateway_motor_receipt: bool,
        asynchronous_mode_completion: bool,
        motor_slot: Option<u8>,
        motor_slots: Vec<u8>,
        gateway_version: Option<Vec<u8>>,
        gateway_stock: Option<Vec<u8>>,
        reject_pc_stop: bool,
        timeout_pc_stop: bool,
        reject_gateway_pc_stop: bool,
        target: Option<u8>,
        closed: bool,
        backup_required: Option<std::path::PathBuf>,
    }
    struct Mock(Rc<RefCell<State>>);
    fn notify(s: &mut State, characteristic: u16, value: Vec<u8>) {
        s.pending.push_back(Notification {
            characteristic,
            value,
        });
    }
    impl Gatt for Mock {
        fn characteristics(&self) -> Vec<CharacteristicInfo> {
            [
                AUTH_CONTROL,
                AUTH_NONCE,
                PCE_COMMAND,
                PCE_RESPONSE,
                FREE_COMMAND,
                FREE_RESPONSE,
                FEATURE,
            ]
            .into_iter()
            .map(|id| CharacteristicInfo {
                id,
                read: true,
                write: true,
                write_without_response: false,
                notify: true,
                indicate: false,
            })
            .collect()
        }
        fn read(&mut self, id: u16) -> Result<Vec<u8>> {
            assert_eq!(id, AUTH_NONCE);
            Ok(if self.0.borrow().invalid_nonce {
                vec![0; 17]
            } else {
                (1..=8).collect()
            })
        }
        fn passkey(&mut self) -> Result<String> {
            Ok("123456".into())
        }
        fn subscribe(&mut self, id: u16) -> Result<()> {
            self.0.borrow_mut().subscriptions.push(id);
            Ok(())
        }
        fn write(&mut self, id: u16, data: &[u8], without_response: bool) -> Result<()> {
            assert!(!without_response);
            let mut s = self.0.borrow_mut();
            s.writes.push((id, data.to_vec()));
            if id == PCE_COMMAND {
                assert_eq!(data[0], 0);
                let response = match data[1] {
                    5 => {
                        let mut reply = vec![0x25];
                        reply.extend(s.gateway_version.clone().unwrap_or(vec![0x46, 0, 0]));
                        reply
                    }
                    0x13 => {
                        assert_eq!(&data[2..], &[1, 0x1c, 0]);
                        let mut reply = vec![0x33];
                        reply.extend(
                            s.gateway_stock
                                .clone()
                                .unwrap_or(vec![1, 0x1e, 0x23, 2, 0xff, 0xff]),
                        );
                        reply
                    }
                    4 if s.master_mode => vec![0x24, 0xa0],
                    4 if s.initial_extended_mode => hex::decode(if s.mode_negotiated {
                        "2493ff00100200f7df80"
                    } else {
                        "2400ff00100000f79f80"
                    })
                    .unwrap(),
                    4 => vec![0x24, if s.bad_mode { 0 } else { 0x83 }],
                    3 => {
                        if data[2] == 0x20 && s.reject_master_mode {
                            notify(&mut s, PCE_RESPONSE, vec![0x23, 1]);
                            return Ok(());
                        }
                        s.master_mode = data[2] == 0x20;
                        s.mode_negotiated = true;
                        vec![
                            0x23,
                            if s.asynchronous_mode_completion && data[2] == 0 {
                                0x10
                            } else {
                                0
                            },
                        ]
                    }
                    6 => {
                        if s.master_mode && data[2] == 19 {
                            notify(&mut s, PCE_RESPONSE, vec![0x26, 1]);
                            return Ok(());
                        }
                        s.target = Some(data[2]);
                        vec![0x26, 0]
                    }
                    0x0c => {
                        if data[2] == 0 && s.timeout_pc_stop {
                            return Ok(());
                        }
                        if data[2] == 1 && s.master_mode && !s.omit_gateway_motor_receipt {
                            let owner = if s.wrong_pc_owner { 19 } else { 0 };
                            let slots = if s.motor_slots.is_empty() {
                                vec![s.motor_slot.unwrap_or(13)]
                            } else {
                                s.motor_slots.clone()
                            };
                            for slot in slots {
                                notify(
                                    &mut s,
                                    FREE_RESPONSE,
                                    vec![slot, 0x32, 0x12, 1, owner, 0xff, 0xff],
                                );
                            }
                        }
                        vec![0x2c, u8::from(data[2] == 0 && s.reject_gateway_pc_stop)]
                    }
                    0x0d => {
                        let mut reply = vec![0x2d];
                        reply.extend(s.master_info.clone().unwrap_or(vec![1, 0xff, 0xff]));
                        reply
                    }
                    0x1b => {
                        let mut reply = vec![0x3b];
                        reply.extend(s.unit_bitmap.unwrap_or([8, 32, 0, 0]));
                        reply
                    } // PC slot 3, motor slot 13
                    0x1d => vec![0x3d, 0, 0],
                    _ => panic!("unexpected gateway command"),
                };
                notify(&mut s, PCE_RESPONSE, response);
            }
            if id == FREE_COMMAND {
                if data[0] == 0 {
                    assert!(
                        s.target.is_none() || s.target == Some(0),
                        "gateway route must be restored after direct unit traffic"
                    );
                } else if !s.master_mode {
                    assert_eq!(
                        s.target,
                        Some(data[0]),
                        "direct unit traffic requires acknowledged target selection"
                    );
                }
                let expected_slot = if matches!((data[1], data[2]), (1 | 2, 0x14))
                    || (data[0] == 0
                        && s.master_info
                            .as_ref()
                            .is_some_and(|info| info.len() >= 3 && info[1] != 0xff))
                {
                    0
                } else {
                    s.motor_slot.unwrap_or(13)
                };
                if expected_slot != 0 && !s.motor_slots.is_empty() {
                    assert!(s.motor_slots.contains(&data[0]));
                } else {
                    assert_eq!(data[0], expected_slot);
                }
                let (group, cmd) = (data[1], data[2]);
                if group == 0x32 && cmd == 0x30 {
                    return Ok(());
                }
                if group == 0x32 && cmd == 0x10 && data[3] == 0 && s.timeout_pc_stop {
                    return Ok(());
                }
                if group == 0x10 && s.timeout_lamp {
                    return Ok(());
                }
                if group == 0x16 && cmd == 0xd8 {
                    for fragment in [
                        vec![data[0], 0x16, 0xda, 0x16, 0, 1, 2, 3, 4, 5],
                        vec![data[0], 0x16, 0xda, 0x26, 6, 7, 8, 9, 10, 11],
                        vec![data[0], 0x16, 0xda, 0x34, 12, 13, 14, 15, 0xff, 0xff],
                    ] {
                        notify(&mut s, FREE_RESPONSE, fragment);
                    }
                    return Ok(());
                }
                if group == 0x16 && cmd == 0xe0 {
                    if data[3] == 0x34 {
                        s.unit_authenticated = true;
                        notify(
                            &mut s,
                            FREE_RESPONSE,
                            vec![data[0], 0x16, 0xe2, 0xff, 0xff, 0xff, 0xff],
                        );
                    }
                    return Ok(());
                }
                if group == 0x33 && matches!(cmd, 0x68 | 0x70) {
                    if data[3] == 0x34 {
                        if cmd == 0x68 {
                            for (selector, offset, length) in
                                [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)]
                            {
                                let mut fragment = vec![data[0], 0x33, 0x6a, selector];
                                fragment.extend(offset..offset + length);
                                fragment.resize(10, 0xff);
                                notify(&mut s, FREE_RESPONSE, fragment);
                            }
                        } else {
                            s.unit_authenticated = true;
                            notify(
                                &mut s,
                                FREE_RESPONSE,
                                vec![data[0], 0x33, 0x72, 0xff, 0xff, 0xff, 0xff],
                            );
                        }
                    }
                    return Ok(());
                }
                let mut response = vec![data[0], group, cmd + 2];
                match (group, cmd) {
                    (0x32, 0x10) => {
                        response.push(if data[3] == 0 && s.reject_pc_stop {
                            1
                        } else {
                            data[3]
                        });
                        if s.master_mode {
                            response.extend([if s.wrong_pc_owner { 19 } else { 0 }, 0xff, 0xff]);
                        }
                    }
                    (0x16, 0x7c) => {
                        response.extend(s.motor_params.clone().unwrap_or(b"00-CRG\0".to_vec()))
                    }
                    (1, 0x1c) => {
                        if let Some(stock) = s.stock_sequence.pop_front() {
                            response.extend(stock);
                        } else if let Some(stock) = s.component_stock.get(&data[0]) {
                            response.extend(stock);
                        } else {
                            response.extend(
                                s.master_stock_reply.clone().unwrap_or(vec![0x23, 0, 0xff]),
                            );
                        }
                    }
                    (1, 0x2c) => {
                        response.extend(s.motor_version.clone().unwrap_or(vec![0x42, 0, 0]))
                    }
                    (1, 0xa4) => response.extend(b"\0ABCDE"),
                    (1, 0xac) => response.extend(b"123456"),
                    (1, 0x84) => response.extend([0x41, 11, 0]),
                    (1, 0xc0) => response.push(data[3]),
                    (1, 0x3c) => response.extend([1, 2, 3, 4, 5, 6]),
                    (1, 0x14) => {
                        response.extend([0, 0]);
                        response.extend(s.unit_bitmap.unwrap_or([8, 32, 0, 0]));
                        response.push(0xff); // observed fixed-size notifications may have extra bytes
                    }
                    (2, 0x14) => {
                        response.push(data[3]);
                        let stock = s.stock_sequence.pop_front().unwrap_or_else(|| {
                            s.component_stock.get(&data[3]).copied().unwrap_or([
                                s.stock_series.unwrap_or(0x23),
                                s.stock_unit.unwrap_or(0),
                            ])
                        });
                        response.extend(stock);
                    }
                    (4, 0x0c) | (0x32, 0x9c) => response.push(s.component_part.unwrap_or(1)),
                    (4, 0x14) => response.push(0x23),
                    (0x0b, 0x64) => response.extend([2, 255]),
                    (0x10, 0x14) if s.reject_lamp => {
                        response[2] += 1;
                    }
                    (0x10, 0x14) => response.push(s.lamp),
                    (0x10, 0x10) => {
                        if let Some(p) = &s.backup_required {
                            let backup: serde_json::Value =
                                serde_json::from_slice(&std::fs::read(p)?).unwrap();
                            assert_eq!(backup["serial"], "010203040506");
                        }
                        s.lamp = data[3];
                    }
                    (0x16, 0xac) => response.extend([data[3], s.destination, 0xff, 0xff]),
                    (0x16, 0xa8) => {
                        if !s.master_mode
                            || s.wrong_pc_owner
                            || !s.unit_authenticated
                            || s.reject_destination_write
                        {
                            response[2] += 1;
                        } else {
                            s.destination = data[4];
                        }
                    }
                    (0x16, 0xb4) => {
                        response.extend_from_slice(&s.speed_limit.to_le_bytes());
                        response.extend([0xff, 0xff]);
                    }
                    (0x16, 0xb0) => {
                        s.speed_limit = u16::from_le_bytes([data[3], data[4]]);
                    }
                    _ => panic!("unexpected unit command {group:02x}/{cmd:02x}"),
                }
                notify(&mut s, FREE_RESPONSE, response);
            }
            Ok(())
        }
        fn receive(&mut self, _: Duration) -> Result<Option<Notification>> {
            Ok(self.0.borrow_mut().pending.pop_front())
        }
        fn close(&mut self) -> Result<()> {
            self.0.borrow_mut().closed = true;
            Ok(())
        }
    }
    fn mock() -> (BleSession, Rc<RefCell<State>>) {
        let state = Rc::new(RefCell::new(State {
            lamp: 1,
            destination: 1,
            speed_limit: 2500,
            ..State::default()
        }));
        (
            BleSession::new(Box::new(Mock(state.clone())), None).unwrap(),
            state,
        )
    }
    fn path(label: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "cranklink-ble-{}-{}-{label}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }
    #[test]
    fn motor_authorization_uses_family_across_firmware_versions_and_variants() {
        for (series, params, modern) in [
            (0x23, &b"00\0\0\0\0\0"[..], false),
            (0x23, &b"00-CRG\0"[..], false),
            (0x30, &b"00\0\0\0\0\0"[..], true),
            (0x30, &b"00-CRG\0"[..], true),
            (0x35, &b"01\0\0\0\0\0"[..], true),
            (0x35, &b"01-CRG\0"[..], true),
            (0x37, &b"00\0\0\0\0\0"[..], true),
        ] {
            for firmware in [match series {
                0x30 => vec![0x44, 1, 0],
                0x35 => vec![0x46, 0, 1],
                _ => vec![0x47, 99, 0],
            }] {
                let (mut session, state) = mock();
                {
                    let mut s = state.borrow_mut();
                    s.master_stock_reply = Some(vec![series, 0, 0xff]);
                    s.motor_params = Some(params.to_vec());
                    s.motor_version = Some(firmware);
                }
                session.set_master_routing(true).unwrap();
                session.authenticate().unwrap();
                session.start_component(Some(13), None).unwrap();
                let receipt = session.authenticate_unit_settings("010203040506").unwrap();
                assert_eq!(
                    receipt["protocol"],
                    if modern { "modern" } else { "legacy" }
                );
                assert!(state.borrow().unit_authenticated);
                let writes = state.borrow();
                assert_eq!(
                    writes
                        .writes
                        .iter()
                        .filter(|(id, bytes)| *id == FREE_COMMAND && bytes[1..3] == [0x33, 0x68])
                        .count(),
                    if modern { 3 } else { 0 }
                );
            }
        }
    }

    #[test]
    fn master_gateway_start_receipt_prevents_duplicate_motor_start() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        let identity = session.start_component(Some(13), None).unwrap();
        assert_eq!(identity.cargo, "00-CRG");
        assert_eq!(identity.dcas_version, "4.2.0.0");
        assert_eq!(identity.rx_version, "4.1.11.0");
        let mismatch = session
            .authenticate_unit_settings("FFFFFFFFFFFF")
            .unwrap_err();
        assert!(mismatch.to_string().contains("motor serial mismatch"));
        assert!(!session.unit_auth_attempted);
        session.authenticate_unit_settings("010203040506").unwrap();
        let writes = &state.borrow().writes;
        for command in [
            vec![13, 1, 0x1c, 0],
            vec![13, 0x16, 0x7c, 0],
            vec![13, 1, 0x2c, 0],
            vec![13, 1, 0x84, 1],
            vec![13, 1, 0x3c, 0],
        ] {
            assert!(writes.contains(&(FREE_COMMAND, command)));
        }
        assert!(!writes
            .iter()
            .any(|(id, bytes)| { *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32]) }));
        assert!(session.gateway_started_slots.is_empty());
        assert!(session.master_pc_start_receipts.is_empty());
    }
    #[test]
    fn master_gateway_start_without_motor_receipt_never_replays_start() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        state.borrow_mut().omit_gateway_motor_receipt = true;
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert!(session.start(Some(13)).is_err());
        assert!(session.poisoned);
        assert!(!state
            .borrow()
            .writes
            .iter()
            .any(|(id, bytes)| { *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32]) }));
    }
    #[test]
    fn browser_command_inventory_preserves_gateway_motor_start_until_selection() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert_eq!(session.units().unwrap().len(), 1);
        assert!(session.linked_slot.is_none());
        session.pause_browser_command().unwrap();
        session.resume_browser_command(true).unwrap();
        assert!(session.gateway_started_slots.contains(&13));
        assert!(session.master_pc_start_receipts.contains_key(&13));
        session.start(Some(13)).unwrap();
        assert!(!state
            .borrow()
            .writes
            .iter()
            .any(|(id, bytes)| { *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32]) }));
        assert!(session.gateway_started_slots.is_empty());
        assert!(session.master_pc_start_receipts.is_empty());
        session.pause_browser_command().unwrap();
        session.stop().unwrap();
    }
    #[test]
    fn browser_command_gateway_receipts_remain_available_for_other_motors() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 96, 0, 0]);
        state.borrow_mut().motor_slots = vec![13, 14];
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert_eq!(session.units().unwrap().len(), 2);
        session.pause_browser_command().unwrap();
        session.resume_browser_command(true).unwrap();
        session.start(Some(13)).unwrap();
        assert!(!session.gateway_started_slots.contains(&13));
        assert!(session.gateway_started_slots.contains(&14));
        assert!(session.master_pc_start_receipts.contains_key(&14));
        session.pause_browser_command().unwrap();
        session.resume_browser_command(true).unwrap();
        session.start(Some(14)).unwrap();
        assert!(!state.borrow().writes.iter().any(|(id, bytes)| {
            *id == FREE_COMMAND
                && bytes[1] == 0x32
                && (bytes[2] == 0x30 || (bytes[2] == 0x10 && bytes[3] == 1))
        }));
        session.pause_browser_command().unwrap();
        session.resume_browser_command(true).unwrap();
        session.start(Some(13)).unwrap();
        let writes = &state.borrow().writes;
        assert_eq!(
            writes
                .iter()
                .filter(|(id, bytes)| {
                    *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32, 0x10, 1])
                })
                .count(),
            1
        );
        assert_eq!(
            writes
                .iter()
                .filter(|(id, bytes)| {
                    *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32, 0x30])
                })
                .count(),
            5
        );
        assert!(session.gateway_started_slots.is_empty());
        assert!(session.master_pc_start_receipts.is_empty());
    }
    fn check_browser_component_switching(master: bool) {
        let (mut session, state) = mock();
        {
            let mut s = state.borrow_mut();
            s.unit_bitmap = Some([if master { 16 } else { 24 }, 32, 0, 0]);
            s.motor_slots = vec![4, 13];
            s.component_stock = BTreeMap::from([(4, [2, 1]), (13, [2, 0])]);
        }
        session.set_master_routing(master).unwrap();
        session.authenticate().unwrap();
        let units = session.units().unwrap();
        assert_eq!(
            units
                .iter()
                .map(|unit| (unit.slot, unit.model))
                .collect::<Vec<_>>(),
            vec![(4, Some("ST-6770-L")), (13, Some("SM-BMR1"))]
        );
        assert!(session.linked_slot.is_none());
        session.pause_browser_command().unwrap();

        session.resume_browser_command(master).unwrap();
        let lever = session.start_component(Some(4), Some("ST-6770-L")).unwrap();
        assert_eq!(lever.cargo, "ST-6770-L");
        let assignment = crate::settings::read_for_model(
            &mut session,
            lever.slot,
            "switch-a-function",
            crate::settings::SettingsModel::Component("ST-6770-L"),
        )
        .unwrap();
        assert_eq!(assignment.value, 2);
        session.pause_browser_command().unwrap();
        assert!(session.linked_slot.is_none());
        if master {
            assert!(session.gateway_started_slots.contains(&13));
            assert!(session.master_pc_start_receipts.contains_key(&13));
        }

        session.resume_browser_command(master).unwrap();
        let power = session.start_component(Some(13), Some("SM-BMR1")).unwrap();
        assert_eq!(power.cargo, "SM-BMR1");
        let limit = crate::settings::read_for_model(
            &mut session,
            power.slot,
            "multi-shift-limit.down",
            crate::settings::SettingsModel::Component("SM-BMR1"),
        )
        .unwrap();
        assert_eq!(limit.value, 255);
        session.pause_browser_command().unwrap();
        assert!(session.linked_slot.is_none());
        session.stop().unwrap();
        session.close().unwrap();

        let writes = &state.borrow().writes;
        assert_eq!(
            writes.iter().filter(|(id, _)| *id == AUTH_CONTROL).count(),
            2
        );
        let stop_lever = writes
            .iter()
            .position(|(id, bytes)| *id == FREE_COMMAND && bytes.starts_with(&[4, 0x32, 0x10, 0]))
            .unwrap();
        let read_power = writes
            .iter()
            .position(|(id, bytes)| *id == FREE_COMMAND && bytes == &[13, 0x0b, 0x64, 0])
            .unwrap();
        let stop_power = writes
            .iter()
            .position(|(id, bytes)| *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32, 0x10, 0]))
            .unwrap();
        let gateway_stop = writes
            .iter()
            .position(|(id, bytes)| *id == PCE_COMMAND && bytes == &[0, 0x0c, 0])
            .unwrap();
        assert!(stop_lever < read_power && read_power < stop_power && stop_power < gateway_stop);
        assert!(!writes.iter().any(|(id, bytes)| *id == FREE_COMMAND
            && (matches!(
                (bytes[1], bytes[2]),
                (0x16, 0x7c | 0xd8 | 0xe0) | (1, 0x84 | 0xc0)
            ) || bytes[1] == 0x33)));
        if master {
            assert_eq!(
                writes
                    .iter()
                    .filter(|(id, bytes)| { *id == PCE_COMMAND && bytes == &[0, 0x1d, 0] })
                    .count(),
                1
            );
            assert_eq!(
                writes
                    .iter()
                    .filter(|(id, bytes)| { *id == PCE_COMMAND && bytes == &[0, 0x0c, 1] })
                    .count(),
                1
            );
            assert!(!writes.iter().any(|(id, bytes)| *id == FREE_COMMAND
                && bytes[1] == 0x32
                && (bytes[2] == 0x30 || (bytes[2] == 0x10 && bytes[3] == 1))));
        }
        assert!(state.borrow().closed);
    }
    #[test]
    fn browser_inventory_then_two_catalogued_settings_reads_preserves_normal_connection() {
        check_browser_component_switching(false);
    }
    #[test]
    fn browser_inventory_then_two_catalogued_settings_reads_preserves_master_receipts() {
        check_browser_component_switching(true);
    }
    #[test]
    fn named_motor_is_rechecked_after_inventory_before_target_start() {
        let (mut session, state) = mock();
        {
            let mut s = state.borrow_mut();
            s.unit_bitmap = Some([0, 32, 0, 0]);
            s.stock_sequence = VecDeque::from([[0x23, 0], [0x30, 0]]);
        }
        session.authenticate().unwrap();
        let error = session
            .start_component(Some(13), Some("DU-EP800-CRG"))
            .unwrap_err();
        assert!(error.to_string().contains("no longer matches slot 13"));
        assert!(session.linked_slot.is_none());
        assert!(!state.borrow().writes.iter().any(|(id, bytes)| {
            *id == FREE_COMMAND
                && (bytes[1] == 0x32 || matches!((bytes[1], bytes[2]), (1, 0x84 | 0xc0)))
        }));
        session.stop().unwrap();
    }
    #[test]
    fn browser_command_trace_replacement_is_atomic_and_redacts_credentials() {
        let (mut session, state) = mock();
        let first = path("first-command.trace");
        let second = path("second-command.trace");
        session.configure_command_trace(Some(&first)).unwrap();
        session.log("tx", PCE_COMMAND, &[0, 4]).unwrap();
        let writes = state.borrow().writes.len();
        assert!(session.configure_command_trace(Some(&first)).is_err());
        assert_eq!(state.borrow().writes.len(), writes);
        session.log("tx", PCE_COMMAND, &[0, 5]).unwrap();
        let first_text = std::fs::read_to_string(&first).unwrap();
        assert!(first_text.contains("0004") && first_text.contains("0005"));
        session.configure_command_trace(Some(&second)).unwrap();
        session.log("tx", AUTH_CONTROL, b"credential").unwrap();
        session.log("rx", AUTH_NONCE, b"challenge").unwrap();
        session
            .log("tx", FREE_COMMAND, &[13, 0x16, 0xd8, 1, 2, 3])
            .unwrap();
        session.log("tx", FREE_COMMAND, &[13, 1, 0x1c, 0]).unwrap();
        let second_text = std::fs::read_to_string(&second).unwrap();
        assert!(second_text.contains("0d011c00"));
        assert!(!second_text.contains("credential") && !second_text.contains("challenge"));
        assert!(!second_text.contains("16d8") && !second_text.contains("2af3"));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), first_text);
        session.configure_command_trace(None).unwrap();
        session.log("tx", PCE_COMMAND, &[0, 4]).unwrap();
        assert_eq!(std::fs::read_to_string(&second).unwrap(), second_text);
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
    }
    #[test]
    fn browser_command_master_handoff_to_firmware_preserves_connection_and_guards() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        session.pause_browser_command().unwrap();
        let auth_count = state
            .borrow()
            .writes
            .iter()
            .filter(|(id, _)| *id == AUTH_CONTROL)
            .count();
        session.resume_browser_command(true).unwrap();
        let identity = session.start(Some(13)).unwrap();
        assert_eq!(identity.rx_version, "4.1.11.0");
        assert!(session.motor_firmware(&[0xff; 6]).is_err());
        assert!(!session.firmware_active);
        assert!(!state
            .borrow()
            .writes
            .iter()
            .any(|(id, bytes)| { *id == FREE_COMMAND && bytes.starts_with(&[13, 1, 0xc0]) }));
        let link = session.motor_firmware(&[1, 2, 3, 4, 5, 6]).unwrap();
        link.application_return().unwrap();
        assert_eq!(
            state
                .borrow()
                .writes
                .iter()
                .filter(|(id, _)| *id == AUTH_CONTROL)
                .count(),
            auth_count
        );
        assert!(!state.borrow().closed);
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0c, 0])));
        session.stop().unwrap();
        session.close().unwrap();
        assert!(state.borrow().closed);
    }
    #[test]
    fn browser_commands_keep_gateway_connected_until_explicit_stop() {
        let (mut session, state) = mock();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        let auth_count = state
            .borrow()
            .writes
            .iter()
            .filter(|(id, _)| *id == AUTH_CONTROL)
            .count();
        session.pause_browser_command().unwrap();
        assert!(!state.borrow().closed);
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0c, 0])));
        session.resume_browser_command(false).unwrap();
        session.start(Some(13)).unwrap();
        assert_eq!(session.command(13, 0x10, 0x14, &[0], 0x16).unwrap()[0], 1);
        session.pause_browser_command().unwrap();
        assert_eq!(
            state
                .borrow()
                .writes
                .iter()
                .filter(|(id, _)| *id == AUTH_CONTROL)
                .count(),
            auth_count
        );
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0c, 0])));
        session.stop().unwrap();
        session.close().unwrap();
        assert!(state.borrow().closed);
        assert_eq!(
            state
                .borrow()
                .writes
                .iter()
                .filter(|(id, bytes)| *id == PCE_COMMAND && bytes == &[0, 0x0c, 0])
                .count(),
            1
        );
    }
    #[test]
    fn browser_commands_retain_master_routing_until_explicit_stop() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        session.pause_browser_command().unwrap();
        session.resume_browser_command(true).unwrap();
        session.start(Some(13)).unwrap();
        assert!(state.borrow().master_mode);
        let writes_before_pause = state.borrow().writes.len();
        session.pause_browser_command().unwrap();
        assert!(state.borrow().master_mode);
        assert!(session.master_routing_active);
        assert!(session.linked_slot.is_none());
        assert_eq!(
            &state.borrow().writes[writes_before_pause..],
            &[(FREE_COMMAND, vec![13, 0x32, 0x10, 0, 0, 0, 0])]
        );
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0c, 0])));
        assert!(session.resume_browser_command(false).is_err());
        let writes_before_resume = state.borrow().writes.len();
        session.resume_browser_command(true).unwrap();
        session.start(Some(13)).unwrap();
        let writes = state.borrow().writes[writes_before_resume..].to_vec();
        assert_eq!(writes[0], (PCE_COMMAND, vec![0, 4]));
        assert_eq!(writes[1], (PCE_COMMAND, vec![0, 0x1b]));
        assert!(!writes.iter().any(|(id, bytes)| {
            *id == PCE_COMMAND && matches!(bytes.get(1), Some(3 | 0x1d | 0x0c))
        }));
        assert!(writes.contains(&(FREE_COMMAND, vec![13, 0x32, 0x10, 1, 0, 0, 0])));
        session.pause_browser_command().unwrap();
        let writes_before_stop = state.borrow().writes.len();
        session.stop().unwrap();
        assert!(!state.borrow().master_mode);
        assert!(!session.master_routing_active);
        assert_eq!(
            &state.borrow().writes[writes_before_stop..],
            &[
                (PCE_COMMAND, vec![0, 3, 0]),
                (PCE_COMMAND, vec![0, 4]),
                (PCE_COMMAND, vec![0, 0x0c, 0]),
            ]
        );
        session.close().unwrap();
        assert!(state.borrow().closed);
    }
    #[test]
    fn browser_command_lost_master_mode_stops_before_motor_commands() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        session.pause_browser_command().unwrap();
        state.borrow_mut().master_mode = false;
        session.resume_browser_command(true).unwrap();
        let before = state.borrow().writes.len();
        let error = session.start(Some(13)).unwrap_err();
        assert!(error.to_string().contains("master routing was lost"));
        assert_eq!(
            &state.borrow().writes[before..],
            &[(PCE_COMMAND, vec![0, 4])]
        );
    }
    #[test]
    fn browser_command_cleanup_failure_fences_session_without_replay() {
        let (mut session, state) = mock();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        state.borrow_mut().timeout_pc_stop = true;
        assert!(session.pause_browser_command().is_err());
        let writes = state.borrow().writes.len();
        assert!(session.resume_browser_command(false).is_err());
        assert_eq!(state.borrow().writes.len(), writes);
        assert!(session.poisoned);
    }
    #[test]
    fn batch_reuses_one_authenticated_master_session_for_two_writes_and_reads() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        let auth_writes = state
            .borrow()
            .writes
            .iter()
            .filter(|(c, _)| *c == AUTH_CONTROL)
            .count();
        let prefix = path("batch");
        let ops: Vec<crate::settings::batch::Operation> =
            serde_json::from_value(serde_json::json!([
                {"op":"set","name":"lamp","value":1},
                {"op":"read","name":"lamp"},
                {"op":"set","name":"lamp","value":0},
                {"op":"read","name":"lamp"}
            ]))
            .unwrap();
        let report = crate::settings::batch::run(
            &mut session,
            13,
            &ops,
            &prefix,
            crate::settings::SettingsModel::Ep800Crg,
        )
        .unwrap();
        assert_eq!(report["completed"], 4);
        assert_eq!(report["results"][1]["value"], 1);
        assert_eq!(report["results"][3]["value"], 0);
        {
            let state = state.borrow();
            assert!(!state.closed);
            assert_eq!(
                state
                    .writes
                    .iter()
                    .filter(|(c, _)| *c == AUTH_CONTROL)
                    .count(),
                auth_writes
            );
            assert_eq!(
                state
                    .writes
                    .iter()
                    .filter(|(c, b)| *c == PCE_COMMAND && b == &[0, 0x0c, 1])
                    .count(),
                1
            );
            assert!(!state
                .writes
                .iter()
                .any(|(c, b)| *c == PCE_COMMAND && b == &[0, 0x0c, 0]));
        }
        session.stop().unwrap();
        session.close().unwrap();
        assert!(state.borrow().closed);
        std::fs::remove_file(report["journal"].as_str().unwrap()).unwrap();
        for i in [0, 2] {
            std::fs::remove_file(report["results"][i]["backup"].as_str().unwrap()).unwrap();
        }
    }
    #[test]
    fn master_routing_discovers_remote_motor_confirms_owner_and_restores_slave() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        let id = session.start_component(None, Some("DU-EP800-CRG")).unwrap();
        assert_eq!(id.slot, 13);
        assert_eq!(session.pc_slot, 0);
        assert_eq!(session.model_identity(13).unwrap(), [13, 0x23, 0]);
        assert!(session.set_master_routing(false).is_err());
        {
            let state = state.borrow();
            let writes = &state.writes;
            assert!(writes.contains(&(PCE_COMMAND, vec![0, 3, 0x20])));
            assert!(writes.contains(&(PCE_COMMAND, vec![0, 0x1d, 0])));
            assert!(writes.contains(&(PCE_COMMAND, vec![0, 0x1b])));
            assert!(!writes
                .iter()
                .any(|(id, bytes)| { *id == FREE_COMMAND && bytes.starts_with(&[13, 0x32]) }));
            assert!(!writes.iter().any(|(c, b)| *c == FREE_COMMAND && b[0] == 0));
            assert!(!writes.iter().any(|(c, b)| *c == PCE_COMMAND && b[1] == 6));
        }
        // A real master-to-slave completion can be 23/10 without a 23/00.
        state.borrow_mut().asynchronous_mode_completion = true;
        session.stop().unwrap();
        assert!(!state.borrow().master_mode);
        assert!(!session.master_routing_active);
        assert_eq!(session.pc_slot, 3);
        let state = state.borrow();
        let unit_stop = state
            .writes
            .iter()
            .rposition(|(c, b)| *c == FREE_COMMAND && b == &[13, 0x32, 0x10, 0, 0, 0, 0])
            .unwrap();
        let restore = state
            .writes
            .iter()
            .rposition(|(c, b)| *c == PCE_COMMAND && b == &[0, 3, 0])
            .unwrap();
        let confirm = state
            .writes
            .iter()
            .rposition(|(c, b)| *c == PCE_COMMAND && b == &[0, 4])
            .unwrap();
        let gateway_stop = state
            .writes
            .iter()
            .rposition(|(c, b)| *c == PCE_COMMAND && b == &[0, 0x0c, 0])
            .unwrap();
        assert!(unit_stop < restore && restore < confirm && confirm < gateway_stop);
        assert_eq!(gateway_stop + 1, state.writes.len());
    }
    #[test]
    fn master_routing_rejects_display_owner_before_privileged_authentication() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        state.borrow_mut().wrong_pc_owner = true;
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert!(session
            .start(Some(13))
            .unwrap_err()
            .to_string()
            .contains("PC owner zero"));
        assert!(
            !state.borrow().writes.iter().any(|(c, b)| *c == FREE_COMMAND
                && b[1] == 0x16
                && matches!(b[2], 0xd8 | 0xe0 | 0xa8 | 0xb0))
        );
        session.stop().unwrap();
        assert!(!state.borrow().master_mode);
        assert!(!session.master_routing_active);
    }
    #[test]
    fn master_routing_can_address_a_motor_reusing_the_old_gateway_slot() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 0, 8, 0]);
        state.borrow_mut().motor_slot = Some(19);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert_eq!(session.start(None).unwrap().slot, 19);
        assert!(!state
            .borrow()
            .writes
            .iter()
            .any(|(c, b)| *c == PCE_COMMAND && b[1] == 6));
        session.stop().unwrap();
    }
    #[test]
    fn rejected_master_mode_does_not_enumerate_or_start_units() {
        let (mut session, state) = mock();
        state.borrow_mut().reject_master_mode = true;
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert!(session
            .discover_units()
            .unwrap_err()
            .to_string()
            .contains("routing rejected"));
        assert!(
            !state
                .borrow()
                .writes
                .iter()
                .any(|(c, b)| *c == FREE_COMMAND
                    || (*c == PCE_COMMAND && matches!(b[1], 0x1d | 0x0c)))
        );
        session.stop().unwrap();
        assert!(!session.master_routing_active);
    }
    #[test]
    fn master_routing_supports_verified_gateway_440_and_restores_slave() {
        let (mut session, state) = mock();
        state.borrow_mut().gateway_version = Some(vec![0x44, 0, 0]);
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        let id = session.start_component(None, Some("DU-EP800-CRG")).unwrap();
        assert_eq!(id.slot, 13);
        assert_eq!(session.pc_slot, 0);
        session.stop().unwrap();
        assert!(!session.master_routing_active);
        assert!(!state.borrow().master_mode);
    }
    #[test]
    fn unlisted_gateway_version_uses_runtime_master_capabilities() {
        let (mut session, state) = mock();
        state.borrow_mut().gateway_version = Some(vec![0x50, 1, 1]);
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert_eq!(session.discover_units().unwrap(), [13]);
        assert_eq!(session.pc_slot, 0);
        session.stop().unwrap();
        assert!(!session.master_routing_active);
    }
    #[test]
    fn alternate_gateway_stock_identities_use_runtime_master_capabilities() {
        for stock in [
            vec![1, 0x1e, 0x31, 2, 0xff, 0xff], // SC-EN600
            vec![1, 0x1e, 0x32, 2, 0xff, 0xff], // SC-EN610
            vec![1, 0x1e, 0x2f, 0, 0xff, 0xff], // EW-EX310
            vec![1, 0x1e, 0x11, 8, 0xff, 0xff], // EW-WU101
        ] {
            let (mut session, state) = mock();
            state.borrow_mut().gateway_stock = Some(stock);
            state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
            session.set_master_routing(true).unwrap();
            session.authenticate().unwrap();
            let id = session.start_component(None, Some("DU-EP800-CRG")).unwrap();
            assert_eq!(id.slot, 13);
            assert_eq!(session.pc_slot, 0);
            session.stop().unwrap();
            assert!(!session.master_routing_active);
            assert!(!state.borrow().master_mode);
        }
    }
    #[test]
    fn invalid_or_truncated_gateway_stock_does_not_change_bus_role() {
        for stock in [
            vec![1, 0x1e, 0x23],
            vec![1, 0x1f, 0x23, 2],
            vec![1, 0x1e, 0, 2],
            vec![1, 0x1e, 0xff, 2],
            vec![1, 0x1e, 0x23, 0xff],
        ] {
            let (mut session, state) = mock();
            state.borrow_mut().gateway_stock = Some(stock);
            session.set_master_routing(true).unwrap();
            session.authenticate().unwrap();
            assert!(session
                .discover_units()
                .unwrap_err()
                .to_string()
                .contains("gateway stock identity"));
            assert!(!session.master_routing_active);
            assert!(!state
                .borrow()
                .writes
                .contains(&(PCE_COMMAND, vec![0, 3, 0x20])));
            session.stop().unwrap();
        }
    }
    #[test]
    fn truncated_gateway_revision_does_not_change_bus_role() {
        let (mut session, state) = mock();
        state.borrow_mut().gateway_version = Some(vec![0x44, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert!(session
            .discover_units()
            .unwrap_err()
            .to_string()
            .contains("truncated"));
        assert!(!session.master_routing_active);
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 3, 0x20])));
        session.stop().unwrap();
    }
    #[test]
    fn rejected_gateway_stop_still_restores_responsive_master_gateway() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        state.borrow_mut().reject_gateway_pc_stop = true;
        let error = session.stop().unwrap_err();
        assert!(format!("{error:#}").contains("gateway PC-link stop rejected"));
        assert!(error.to_string().contains("slave routing was restored"));
        assert!(!session.master_routing_active);
        assert!(!state.borrow().master_mode);
        assert_eq!(
            state
                .borrow()
                .writes
                .iter()
                .filter(|(c, b)| *c == PCE_COMMAND && b == &[0, 0x0c, 0])
                .count(),
            1
        );
    }
    #[test]
    fn master_stop_timeout_never_claims_restore_or_dispatches_more_commands() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        session.start(Some(13)).unwrap();
        state.borrow_mut().timeout_pc_stop = true;
        let before = state.borrow().writes.len();
        assert!(session
            .stop()
            .unwrap_err()
            .to_string()
            .contains("was not confirmed"));
        assert!(session.poisoned && session.master_routing_active);
        assert_eq!(state.borrow().writes.len(), before + 1);
    }
    #[test]
    fn empty_master_inventory_restores_slave_without_starting_pc_link() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([0; 4]);
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        assert!(session
            .discover_units()
            .unwrap_err()
            .to_string()
            .contains("no remote components"));
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0c, 1])));
        session.stop().unwrap();
        assert!(!state.borrow().master_mode);
    }
    #[test]
    fn auth_vectors_match_fips_and_independent_aes_pkcs7_calculation() {
        let key: Vec<u8> = (0..16).collect();
        let nonce = hex::decode("00112233445566778899aabbccddeeff").unwrap();
        assert_eq!(
            hex::encode(encrypt_challenge(&key, &nonce).unwrap()),
            "69c4e0d86a7b0430d8cdb78070b4c55a"
        );
        // Independently calculated using Python cryptography AES/ECB with PKCS7.
        let (user, app) = auth_packets("123456", &(1..=8).collect::<Vec<_>>()).unwrap();
        assert_eq!(hex::encode(user), "01ad1684bdf540ea3a");
        assert_eq!(hex::encode(app), "02991f77291a7accbd");
        for pin in ["12345", "1234567", "１２３４５６", "12345x"] {
            assert!(auth_packets(pin, &[0; 8]).is_err());
        }
        assert!(encrypt_challenge(&key, &[]).is_err());
        assert!(encrypt_challenge(&key, &[0; 17]).is_err());
    }
    #[test]
    fn authentication_requires_gateway_receipt_and_redacts_authentication_trace() {
        let (mut s, state) = mock();
        let trace = path("trace.jsonl");
        s.trace = Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&trace)
                .unwrap(),
        );
        s.authenticate().unwrap();
        let writes = &state.borrow().writes;
        assert_eq!(
            writes[0],
            (AUTH_CONTROL, hex::decode("01ad1684bdf540ea3a").unwrap())
        );
        assert_eq!(
            writes[1],
            (AUTH_CONTROL, hex::decode("02991f77291a7accbd").unwrap())
        );
        assert_eq!(writes[2], (FEATURE, vec![0xff, 0]));
        assert!(state.borrow().subscriptions.contains(&FREE_RESPONSE));
        let output = std::fs::read_to_string(&trace).unwrap();
        assert!(
            !output.contains("2af3")
                && !output.contains("2af4")
                && !output.contains("ad1684")
                && !output.contains("123456")
        );
        std::fs::remove_file(trace).unwrap();
        let (mut bad, state) = mock();
        state.borrow_mut().bad_mode = true;
        assert!(bad.authenticate().is_err());
        assert!(!bad.authenticated);
        let (mut bad, state) = mock();
        state.borrow_mut().invalid_nonce = true;
        assert!(bad.authenticate().is_err());
        assert!(state.borrow().writes.is_empty());
    }
    #[test]
    fn authentication_negotiates_normal_slave_before_querying_initial_gateway_state() {
        let (mut session, state) = mock();
        state.borrow_mut().initial_extended_mode = true;
        session.authenticate().unwrap();
        let writes: Vec<_> = state
            .borrow()
            .writes
            .iter()
            .filter(|(id, _)| *id == PCE_COMMAND)
            .cloned()
            .collect();
        assert_eq!(
            writes,
            vec![(PCE_COMMAND, vec![0, 3, 0]), (PCE_COMMAND, vec![0, 4])]
        );
        assert_eq!(session.pc_slot, 19);
        state.borrow_mut().unit_bitmap = Some([0, 32, 8, 0]); // motor 13 and PC 19
        assert_eq!(session.discover_units().unwrap(), vec![13]);
        session.stop().unwrap();
        assert!(!state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x1b])));
    }
    #[test]
    fn discovery_includes_verified_master_zero_missing_from_slave_bitmap() {
        let (mut session, state) = mock();
        state.borrow_mut().initial_extended_mode = true;
        // Physical gateway reports compatibility=1, independently of unit=0.
        state.borrow_mut().master_info = Some(vec![1, 0x23, 1, 0, 0, 0, 0xf7, 0x9f, 0x80]);
        state.borrow_mut().master_stock_reply = Some(vec![0x23, 0, 0xff]);
        state.borrow_mut().unit_bitmap = Some([0x10, 0x20, 0x08, 0x08]);
        session.authenticate().unwrap();
        assert_eq!(session.discover_units().unwrap(), vec![0, 4, 13, 27]);
        assert_eq!(session.model_identity(0).unwrap(), [0, 0x23, 0]);
        assert!(state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 6, 0])));
        assert!(state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0d, 0])));
        assert!(!state
            .borrow()
            .writes
            .contains(&(FREE_COMMAND, vec![0, 2, 0x14, 0])));
        assert!(session.start(Some(0)).is_ok());
        session.stop().unwrap();
        assert!(!state
            .borrow()
            .writes
            .iter()
            .any(|(id, bytes)| *id == FREE_COMMAND
                && bytes[0] == 0
                && bytes[1] == 0x32
                && matches!(bytes[2], 0x10 | 0x30)));
        assert!(state
            .borrow()
            .writes
            .contains(&(PCE_COMMAND, vec![0, 0x0c, 0])));
        assert!(session.linked_slot.is_none());
    }

    #[test]
    fn master_identity_never_survives_protocol_renegotiation_or_absence() {
        let (mut session, state) = mock();
        session.authenticated = true;
        session.gateway_mode = true;
        session.master_stock = Some([0x23, 0]);
        session.gateway(3, &[0], 0x23).unwrap();
        let writes = state.borrow().writes.len();
        assert!(session.model_identity(0).is_err());
        assert_eq!(state.borrow().writes.len(), writes);
        assert_eq!(session.discover_units().unwrap(), vec![13]);
        assert!(session.model_identity(0).is_err());
        session.stop().unwrap();
    }

    #[test]
    fn malformed_or_unpowered_master_identity_stops_discovery() {
        for reply in [vec![1, 0x23], vec![0, 0x23, 0]] {
            let (mut session, state) = mock();
            state.borrow_mut().master_info = Some(reply);
            session.authenticated = true;
            assert!(session.discover_units().is_err());
            assert!(session.master_stock.is_none());
            assert!(!state
                .borrow()
                .writes
                .iter()
                .any(|(id, _)| *id == FREE_COMMAND));
            session.stop().unwrap();
        }
    }

    #[test]
    fn master_stock_read_rejects_truncation_or_gateway_series_mismatch() {
        for reply in [vec![0x23], vec![0x30, 0]] {
            let (mut session, state) = mock();
            state.borrow_mut().master_info = Some(vec![1, 0x23, 1]);
            state.borrow_mut().master_stock_reply = Some(reply);
            session.authenticated = true;
            assert!(session.discover_units().is_err());
            assert!(session.master_stock.is_none());
            let free: Vec<_> = state
                .borrow()
                .writes
                .iter()
                .filter(|(id, _)| *id == FREE_COMMAND)
                .cloned()
                .collect();
            assert_eq!(free, [(FREE_COMMAND, vec![0, 1, 0x1c, 0])]);
            session.stop().unwrap();
        }
    }

    #[test]
    fn gateway_cache_queries_restore_target_after_direct_component_reads() {
        let (mut session, state) = mock();
        session.authenticated = true;
        session.gateway_mode = true;
        session.pc_slot = 3;
        session.command(13, 0x16, 0x7c, &[0], 0x7e).unwrap();
        session.model_identity(13).unwrap();
        session.command(13, 0x16, 0x7c, &[0], 0x7e).unwrap();
        let targets: Vec<_> = state
            .borrow()
            .writes
            .iter()
            .filter(|(id, bytes)| *id == PCE_COMMAND && bytes[1] == 6)
            .map(|(_, bytes)| bytes[2])
            .collect();
        assert_eq!(targets, vec![13, 0, 13]);
    }
    #[test]
    fn mixed_component_selection_refuses_ambiguity_and_non_motor_before_link_start() {
        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([24, 32, 0, 0]); // PC 3, component 4, motor 13
        session.authenticate().unwrap();
        assert!(session
            .start(None)
            .unwrap_err()
            .to_string()
            .contains("select --slot"));
        assert!(state
            .borrow()
            .writes
            .iter()
            .filter(|(id, _)| *id == FREE_COMMAND)
            .all(|(_, bytes)| bytes == &[0, 1, 0x14, 0] || bytes.starts_with(&[0, 2, 0x14])));
        session.stop().unwrap();

        let (mut session, state) = mock();
        state.borrow_mut().unit_bitmap = Some([24, 32, 0, 0]);
        state.borrow_mut().stock_series = Some(0x99);
        session.authenticate().unwrap();
        assert!(session
            .start(Some(4))
            .unwrap_err()
            .to_string()
            .contains("not a recognized motor"));
        let unit_writes: Vec<_> = state
            .borrow()
            .writes
            .iter()
            .filter(|(id, _)| *id == FREE_COMMAND)
            .cloned()
            .collect();
        assert_eq!(
            unit_writes,
            vec![
                (FREE_COMMAND, vec![0, 1, 0x14, 0]),
                (FREE_COMMAND, vec![0, 2, 0x14, 4])
            ]
        );
        session.stop().unwrap();
        assert!(session.linked_slot.is_none());
    }
    fn component_mock(series: u8, unit: u8) -> (BleSession, Rc<RefCell<State>>) {
        let (mut session, state) = mock();
        {
            let mut s = state.borrow_mut();
            s.unit_bitmap = Some([24, 0, 0, 0]); // PC 3, component 4
            s.stock_series = Some(series);
            s.stock_unit = Some(unit);
            s.motor_slot = Some(4);
        }
        session.authenticate().unwrap();
        (session, state)
    }
    fn check_component_start(series: u8, unit: u8, model: &str) {
        let (mut session, state) = component_mock(series, unit);
        let identity = session.start_component(Some(4), None).unwrap();
        assert_eq!(identity.cargo, model);
        assert_eq!(identity.dcas_version, "4.2.0.0");
        assert_eq!(identity.rx_version, "not applicable");
        session.stop().unwrap();
        let writes = &state.borrow().writes;
        assert!(writes.contains(&(FREE_COMMAND, vec![4, 0x32, 0x10, 1, 3, 0, 0])));
        assert!(writes.contains(&(FREE_COMMAND, vec![4, 1, 0x2c, 0])));
        assert!(writes.contains(&(FREE_COMMAND, vec![4, 0x32, 0x10, 0, 3, 0, 0])));
        assert!(!writes.iter().any(|(id, bytes)| *id == FREE_COMMAND
            && matches!((bytes[1], bytes[2]), (0x16, 0x7c) | (1, 0x84))));
    }
    #[test]
    fn motor_start_with_explicit_slot_refuses_a_component_before_pc_link() {
        let (mut session, state) = component_mock(2, 4);
        assert!(session.start(Some(4)).is_err());
        let writes = &state.borrow().writes;
        assert!(!writes.iter().any(|(id, bytes)| *id == FREE_COMMAND
            && bytes.get(1..3) == Some(&[0x32, 0x10][..])
            && bytes.get(3) == Some(&1)));
    }
    #[test]
    fn explicit_derailleur_ble_session_skips_motor_attributes_and_rx_version() {
        check_component_start(2, 4, "RD-6770");
    }
    #[test]
    fn explicit_power_unit_ble_session_skips_motor_attributes_and_rx_version() {
        check_component_start(2, 0, "SM-BMR1");
    }
    #[test]
    fn explicit_hub_actuator_ble_session_skips_motor_attributes_and_rx_version() {
        check_component_start(6, 4, "MU-S705");
    }
    #[test]
    fn named_lever_ble_session_resolves_part_before_target_start() {
        let (mut session, state) = component_mock(2, 1);
        let identity = session.start_component(Some(4), Some("ST-6770-L")).unwrap();
        assert_eq!(identity.cargo, "ST-6770-L");
        assert_eq!(identity.rx_version, "not applicable");
        let error = session
            .authenticate_unit_settings("010203040506")
            .unwrap_err();
        assert!(error.to_string().contains("not a recognized motor"));
        session.stop().unwrap();
        let writes = &state.borrow().writes;
        let part = writes
            .iter()
            .position(|(id, bytes)| *id == FREE_COMMAND && bytes == &[4, 4, 0x0c, 0])
            .unwrap();
        let start = writes
            .iter()
            .position(|(id, bytes)| *id == FREE_COMMAND && bytes == &[4, 0x32, 0x10, 1, 3, 0, 0])
            .unwrap();
        assert!(part < start);
        assert!(!writes.iter().any(|(id, bytes)| *id == FREE_COMMAND
            && matches!((bytes[1], bytes[2]), (0x16, 0x7c) | (1, 0x84))));
    }
    #[test]
    fn named_component_ble_selection_intersects_explicit_slot() {
        let (mut session, state) = component_mock(0x23, 2);
        state.borrow_mut().unit_bitmap = Some([24, 32, 0, 0]);
        assert!(session
            .start_component(Some(4), Some("RD-6770"))
            .unwrap_err()
            .to_string()
            .contains("matching slots []"));
        assert!(!state.borrow().writes.iter().any(|(id, bytes)| {
            *id == FREE_COMMAND && matches!((bytes[1], bytes[2]), (0x32, 0x10 | 0x30))
        }));
        assert!(session.linked_slot.is_none());
        session.stop().unwrap();
    }
    #[test]
    fn unknown_explicit_ble_component_is_rejected_before_target_start() {
        let (mut session, state) = component_mock(0x99, 2);
        assert!(session
            .start_component(Some(4), None)
            .unwrap_err()
            .to_string()
            .contains("not a catalogued component"));
        assert!(!state.borrow().writes.iter().any(|(id, bytes)| {
            *id == FREE_COMMAND && matches!((bytes[1], bytes[2]), (0x32, 0x10 | 0x30))
        }));
        assert!(session.linked_slot.is_none());
        session.stop().unwrap();
    }
    #[test]
    fn unresolved_explicit_ble_lever_is_rejected_before_target_start() {
        let (mut session, state) = component_mock(2, 1);
        state.borrow_mut().component_part = Some(0xff);
        assert!(session
            .start_component(Some(4), None)
            .unwrap_err()
            .to_string()
            .contains("component model remains unresolved"));
        assert!(!state.borrow().writes.iter().any(|(id, bytes)| {
            *id == FREE_COMMAND && matches!((bytes[1], bytes[2]), (0x32, 0x10 | 0x30))
        }));
        assert!(session.linked_slot.is_none());
        session.stop().unwrap();
    }
    #[test]
    fn default_ble_start_remains_motor_only_for_known_component() {
        let (mut session, state) = component_mock(2, 4);
        assert!(session
            .start(None)
            .unwrap_err()
            .to_string()
            .contains("not a recognized motor"));
        assert!(!state.borrow().writes.iter().any(|(id, bytes)| {
            *id == FREE_COMMAND && matches!((bytes[1], bytes[2]), (0x32, 0x10 | 0x30))
        }));
        assert!(session.linked_slot.is_none());
        session.stop().unwrap();
    }
    #[test]
    fn explicit_component_master_routing_uses_existing_gateway_start_receipt() {
        let (mut session, state) = mock();
        {
            let mut s = state.borrow_mut();
            s.unit_bitmap = Some([0, 32, 0, 0]);
            s.master_stock_reply = Some(vec![2, 0, 0xff]);
        }
        session.set_master_routing(true).unwrap();
        session.authenticate().unwrap();
        let identity = session.start_component(Some(13), None).unwrap();
        assert_eq!(identity.cargo, "SM-BMR1");
        assert_eq!(identity.rx_version, "not applicable");
        assert!(session
            .motor_firmware(&[1, 2, 3, 4, 5, 6])
            .err()
            .unwrap()
            .to_string()
            .contains("only EP800"));
        assert!(session
            .motor_firmware_with_policy(&[1, 2, 3, 4, 5, 6], true)
            .err()
            .unwrap()
            .to_string()
            .contains("does not support motor"));
        let writes = &state.borrow().writes;
        assert!(writes.contains(&(FREE_COMMAND, vec![13, 1, 0x2c, 0])));
        assert!(!writes.iter().any(|(id, bytes)| *id == FREE_COMMAND
            && (matches!((bytes[1], bytes[2]), (0x16, 0x7c) | (1, 0x84 | 0xc0))
                || bytes.starts_with(&[13, 0x32]))));
    }
    #[test]
    fn discovery_pc_link_identity_and_stop_use_gatt_channels_without_usb_frames() {
        let (mut s, state) = mock();
        s.authenticate().unwrap();
        let id = s.start(None).unwrap();
        assert_eq!(id.slot, 13);
        assert_eq!(id.cargo, "00-CRG");
        assert_eq!(id.rx_version, "4.1.11.0");
        s.stop().unwrap();
        s.close().unwrap();
        let state = state.borrow();
        assert!(state.closed);
        assert!(state
            .writes
            .contains(&(FREE_COMMAND, vec![13, 0x32, 0x10, 1, 3, 0, 0])));
        assert!(state
            .writes
            .contains(&(FREE_COMMAND, vec![13, 0x32, 0x10, 0, 3, 0, 0])));
        assert!(state.writes.iter().all(|(_, bytes)| bytes.len() <= 20));
        assert_eq!(
            state.writes.last().unwrap(),
            &(PCE_COMMAND, vec![0, 0x0c, 0])
        );
    }
    #[test]
    fn settings_write_backs_up_before_dispatch_and_restores_original() {
        let (mut s, state) = mock();
        s.authenticate().unwrap();
        s.gateway_mode = true;
        let backup = path("before.json");
        let restore_backup = path("restore.json");
        state.borrow_mut().backup_required = Some(backup.clone());
        let receipt = crate::settings::set(&mut s, 13, "lamp", "2", &backup).unwrap();
        assert!(receipt.changed);
        assert_eq!(state.borrow().lamp, 2);
        // A failed backup creation must not result in another SET.
        let before = state.borrow().writes.len();
        assert!(crate::settings::set(&mut s, 13, "lamp", "1", &backup).is_err());
        assert!(state.borrow().writes[before..]
            .iter()
            .all(|(id, b)| *id != FREE_COMMAND || b[2] != 0x10));
        state.borrow_mut().backup_required = Some(restore_backup.clone());
        crate::settings::restore(&mut s, 13, &backup, &restore_backup).unwrap();
        assert_eq!(state.borrow().lamp, 1);
        std::fs::remove_file(backup).unwrap();
        std::fs::remove_file(restore_backup).unwrap();
    }
    #[test]
    fn motor_errors_are_reported_and_timeout_poison_prevents_later_writes() {
        let (mut s, state) = mock();
        s.authenticate().unwrap();
        s.gateway_mode = true;
        // Stale successful replies must not replace the new error response.
        notify(
            &mut state.borrow_mut(),
            FREE_RESPONSE,
            vec![3, 0x10, 0x16, 1],
        );
        state.borrow_mut().reject_lamp = true;
        assert!(crate::settings::read(&mut s, 13, "lamp")
            .unwrap_err()
            .to_string()
            .contains("rejected"));
        assert!(!s.poisoned);
        state.borrow_mut().timeout_lamp = true;
        assert!(crate::settings::read(&mut s, 13, "lamp").is_err());
        assert!(s.poisoned);
        let before = state.borrow().writes.len();
        assert!(crate::settings::read(&mut s, 13, "lamp").is_err());
        assert_eq!(state.borrow().writes.len(), before);
    }
    #[test]
    fn firmware_and_mode_switching_are_rejected_before_gatt_dispatch() {
        let (mut s, state) = mock();
        s.authenticated = true;
        s.gateway_mode = true;
        for (g, c) in [(1, 0xc0), (0x32, 0x30), (0xff, 0xff)] {
            assert!(s.command(13, g, c, &[0], c.wrapping_add(2)).is_err());
        }
        assert!(s.gateway(7, &[6], 0x27).is_err());
        assert!(s.gateway(0x13, &[1, 0xc0, 0], 0x33).is_err());
        assert!(s.gateway(0x13, &[0x16, 0xa8, 1, 1], 0x33).is_err());
        assert!(s.write(0x2afc, &[1, 2, 3]).is_err());
        assert!(s.write(FEATURE, &[0, 0x80]).is_err());
        assert!(state.borrow().writes.is_empty());
        assert!(crate::device::Session::open("ble:any", None).is_err());
    }
    #[test]
    fn post_auth_master_routing_authenticates_ep800_runs_mixed_destination_speed_batch_and_redacts_trace(
    ) {
        let (mut session, state) = mock();
        let trace = path("post-auth-master-trace.jsonl");
        session.trace = Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&trace)
                .unwrap(),
        );
        state.borrow_mut().unit_bitmap = Some([0, 32, 0, 0]);
        // Resolve master routing after gateway authentication, before component discovery.
        session.authenticate().unwrap();
        assert!(!session.is_master_routing());
        session.set_master_routing(true).unwrap();
        assert!(session.is_master_routing());
        let id = session.start_component(None, Some("DU-EP800-CRG")).unwrap();
        assert_eq!(id.slot, 13);
        assert_eq!(session.pc_slot, 0);
        // After discovery, changing routing policy on the active session is rejected.
        assert!(session.set_master_routing(false).is_err());

        session.authenticate_unit_settings("010203040506").unwrap();

        let prefix = path("mixed-dest-speed");
        let ops: Vec<crate::settings::batch::Operation> =
            serde_json::from_value(serde_json::json!([
                {"op":"set","name":"destination","value":3},
                {"op":"set","name":"speed-limit","value":25}
            ]))
            .unwrap();
        let report = crate::settings::batch::run(
            &mut session,
            13,
            &ops,
            &prefix,
            crate::settings::SettingsModel::Ep800Crg,
        )
        .unwrap();
        assert_eq!(report["completed"], 2);
        assert_eq!(state.borrow().destination, 3);
        assert_eq!(state.borrow().speed_limit, 2500);

        session.stop().unwrap();
        session.close().unwrap();
        assert!(!session.master_routing_active);
        assert!(!state.borrow().master_mode);

        let trace_text = std::fs::read_to_string(&trace).unwrap();
        assert!(
            !trace_text.contains("2af3")
                && !trace_text.contains("2af4")
                && !trace_text.contains("16d8")
                && !trace_text.contains("16da")
                && !trace_text.contains("16e0")
                && !trace_text.contains("16e2"),
            "gateway and drive-unit authentication material must remain absent from trace"
        );
        std::fs::remove_file(trace).unwrap();
        std::fs::remove_file(report["journal"].as_str().unwrap()).unwrap();
        for i in [0, 1] {
            std::fs::remove_file(report["results"][i]["backup"].as_str().unwrap()).unwrap();
        }
    }
    #[test]
    fn rejected_destination_write_stops_without_route_retry_or_second_write() {
        // Explicit normal routing or motor rejection must fail on the first attempt without retrying.
        let (mut session, state) = mock();
        session.authenticate().unwrap();
        session.set_master_routing(false).unwrap();
        let id = session.start(Some(13)).unwrap();
        session.authenticate_unit_settings("010203040506").unwrap();
        let backup = path("rejected-dest-backup.json");
        let err = crate::settings::set_for_model(
            &mut session,
            id.slot,
            "destination",
            "3",
            &backup,
            crate::settings::SettingsModel::Ep800Crg,
        )
        .unwrap_err();
        assert!(err.to_string().contains("setting write failed"));
        let dest_writes = state
            .borrow()
            .writes
            .iter()
            .filter(|(c, b)| *c == FREE_COMMAND && b.starts_with(&[13, 0x16, 0xa8]))
            .count();
        assert_eq!(dest_writes, 1, "must never retry a rejected write");
        assert!(
            !state
                .borrow()
                .writes
                .contains(&(PCE_COMMAND, vec![0, 3, 0x20])),
            "must never switch routes after a failed write"
        );
        session.stop().unwrap();
        std::fs::remove_file(backup).unwrap();
    }
}
