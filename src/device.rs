//! Exclusive transport session. Commands are serialized and full frames journaled.
//!
//! `Session::units` discovers arbitrary catalogued component types and refines
//! their stock identities without motor PC-link startup, re-initializing the
//! adapter first when this connection already ran discovery. Explicit component
//! probes and settings require a unique catalogued identity; default startup
//! and firmware paths retain their motor gates and application/boot checks.
//! A resolved name alone does not grant every operation. Normal routed requests carry
//! the target slot, while control-0x48 replies carry this session's PC slot.
use crate::protocol::{self, Decoder, Frame};
use crate::{
    platform::{self, sleep, Instant, Port},
    storage::{File, OpenOptions},
};
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
#[derive(Debug, Serialize)]
pub struct PowerInfo {
    pub adapter_supply: bool,
    pub raw_state: u8,
}
#[derive(Debug, Serialize, Clone)]
pub struct Identity {
    pub slot: u8,
    /// Motor attribute discriminator, or the resolved model for other components.
    pub cargo: String,
    /// Application firmware version (the historical DCAS field for motors).
    pub dcas_version: String,
    pub rx_version: String,
}
/// Stock identity is readable before entering a component PC-link session.
#[derive(Debug, Serialize)]
pub struct UnitIdentity {
    pub slot: u8,
    pub model_series: u8,
    pub model_unit: u8,
    pub model: Option<&'static str>,
    pub model_candidates: Vec<&'static str>,
    pub part: Option<u8>,
    pub model_params: Option<Vec<u8>>,
    pub raw: Vec<u8>,
}
impl UnitIdentity {
    pub fn require_motor(&self) -> Result<()> {
        ensure!(self.is_motor(), "slot {} is not a recognized motor (series {:02X}, unit {:02X}); use units to inspect components", self.slot, self.model_series, self.model_unit);
        Ok(())
    }
    pub fn is_motor(&self) -> bool {
        matches!(
            (self.model_series, self.model_unit),
            (
                0x08 | 0x0a | 0x0c | 0x12 | 0x15 | 0x20 | 0x21 | 0x23 | 0x30 | 0x35 | 0x37 | 0x38,
                0
            )
        )
    }
    pub fn resolve_name(&mut self, commands: &mut impl UnitCommands) -> Result<()> {
        let resolved = crate::unit_models::read_identification(
            commands,
            self.slot,
            self.model_series,
            self.model_unit,
        )?;
        self.model = resolved.model;
        self.model_candidates = resolved.model_candidates;
        self.part = resolved.part;
        self.model_params = resolved.model_params;
        Ok(())
    }
    /// Motors retain their existing admission rules. Other components require
    /// explicit selection and a unique catalogue identity before PC-link start.
    pub(crate) fn prepare_session(
        &mut self,
        commands: &mut impl UnitCommands,
        allow_component: bool,
    ) -> Result<()> {
        if self.is_motor() {
            return Ok(());
        }
        if !allow_component {
            return self.require_motor();
        }
        ensure!(
            !self.model_candidates.is_empty(),
            "slot {} is not a catalogued component (series {:02X}, unit {:02X}); inspect units before starting a settings session",
            self.slot, self.model_series, self.model_unit
        );
        // A drive unit outside the motor families (e.g. DU-E5000) must not
        // take the generic component path with its motor-specific services.
        ensure!(
            !self
                .model_candidates
                .iter()
                .any(|name| name.starts_with("DU-")),
            "slot {} is an unsupported drive unit ({}); no settings session is available",
            self.slot,
            self.model_candidates.join(" / ")
        );
        if self.model.is_none() {
            self.resolve_name(commands)?;
        }
        ensure!(
            self.model.is_some(),
            "slot {} component model remains unresolved; candidates {:?}; inspect units before starting a settings session",
            self.slot, self.model_candidates
        );
        Ok(())
    }
    pub fn from_reply(slot: u8, reply: Vec<u8>) -> Result<Self> {
        ensure!(
            reply.len() >= 3 && reply[0] == slot,
            "invalid unit stock identity for slot {slot}"
        );
        let model_candidates = crate::unit_models::candidates(reply[1], reply[2], None, None);
        let model = (model_candidates.len() == 1).then(|| model_candidates[0]);
        Ok(Self {
            slot,
            model_series: reply[1],
            model_unit: reply[2],
            model,
            model_candidates,
            part: None,
            model_params: None,
            raw: reply[1..].to_vec(),
        })
    }
}
#[derive(Debug, Serialize)]
pub struct ProbeIdentity {
    #[serde(flatten)]
    pub identity: Identity,
    pub serial: String,
    pub model_series: u8,
    pub model_unit: u8,
}
#[derive(Debug, Clone, Serialize)]
pub struct BootIdentity {
    pub slot: u8,
    pub version: [u8; 4],
    pub board: u8,
    pub series: u8,
    pub unit: u8,
    pub serial: [u8; 6],
}
#[derive(Debug, Serialize)]
pub struct FlashReceipt {
    pub bytes: usize,
    pub checksum: u8,
    pub finish_acknowledged: bool,
    pub reset_acknowledged: bool,
}
/// Canonical DCAS identity, excluding the reserved FF byte in stock replies.
/// Existing six-byte fixtures and physical seven-byte EP800 replies are accepted.
pub fn serial_bytes(parameters: &[u8]) -> Result<[u8; 6]> {
    ensure!(
        parameters.len() == 6 || (parameters.len() == 7 && parameters[6] == 0xff),
        "invalid motor serial reply length or reserved byte"
    );
    Ok(parameters[..6].try_into().unwrap())
}
/// Generic unit serial GET declares six identity bytes. Classic components may
/// return an additional transport byte whose value is not reserved FF.
/// Motor authentication and firmware identity must keep using `serial_bytes`.
pub fn component_serial_bytes(parameters: &[u8]) -> Result<[u8; 6]> {
    ensure!(
        matches!(parameters.len(), 6 | 7),
        "invalid component serial reply length"
    );
    Ok(parameters[..6].try_into().unwrap())
}
/// Logical settings commands; transports own their framing and session setup.
pub trait UnitCommands {
    /// Slot-stock query goes to the gateway in BLE slave mode.
    fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
        self.command(0, 2, 0x14, &[slot], 0x16)
    }
    fn command(
        &mut self,
        slot: u8,
        group: u8,
        command: u8,
        params: &[u8],
        response_cmd: u8,
    ) -> Result<Vec<u8>>;
}
impl UnitCommands for Session {
    /// Read the component directly: SM-PCE02 does not answer the BLE gateway query.
    fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
        let stock = self.command(slot, 1, 0x1c, &[0], 0x1e)?;
        ensure!(stock.len() >= 2, "truncated component stock identity");
        Ok(vec![slot, stock[0], stock[1]])
    }
    fn command(
        &mut self,
        slot: u8,
        group: u8,
        command: u8,
        params: &[u8],
        response_cmd: u8,
    ) -> Result<Vec<u8>> {
        Session::command(self, slot, group, command, params, response_cmd)
    }
}
pub struct Session {
    port: Box<dyn Port>,
    decoder: Decoder,
    incoming: VecDeque<Frame>,
    trace: Option<File>,
    pc_slot: u8,
    normal_slot: Option<u8>,
    normal_is_motor: bool,
    target_slot: Option<u8>,
    adapter_supply: bool,
    unit_auth_attempted: bool,
    discovery: DiscoveryState,
}
/// Where the next discovery starts relative to the last adapter
/// initialization; see `reinitialize_for_unlinked_identity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryState {
    /// No discovery since the last completed `initialize` (or since opening).
    Fresh,
    /// Discovery has run since the last completed `initialize`.
    Repeated,
    /// An adapter reset was requested and `initialize` has not completed since,
    /// so the adapter's power and protocol state are unknown.
    ResetIncomplete,
}
impl Session {
    /// Continue the same exclusive USB connection after the bounded bridge
    /// handshake. The caller has consumed every received frame and selected
    /// target 31; initialization or reopening here would lose that state.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_bridge_port(port: Box<dyn Port>, trace: File) -> Self {
        Self {
            port,
            decoder: Decoder::default(),
            incoming: VecDeque::new(),
            trace: Some(trace),
            pc_slot: 0,
            normal_slot: None,
            normal_is_motor: false,
            target_slot: Some(31),
            adapter_supply: true,
            unit_auth_attempted: false,
            discovery: DiscoveryState::Fresh,
        }
    }
    pub fn open(port: &str, trace: Option<&Path>) -> Result<Self> {
        ensure!(!crate::ble::is_port(port), "Bluetooth requires the settings-only BLE session; firmware/boot operations are disabled");
        let port = platform::open_port(port)?;
        let trace = trace
            .map(|p| OpenOptions::new().create(true).append(true).open(p))
            .transpose()?;
        Ok(Self {
            port,
            decoder: Decoder::default(),
            incoming: VecDeque::new(),
            trace,
            pc_slot: 0,
            normal_slot: None,
            normal_is_motor: false,
            target_slot: None,
            adapter_supply: false,
            unit_auth_attempted: false,
            discovery: DiscoveryState::Fresh,
        })
    }
    fn log(&mut self, direction: &str, bytes: &[u8]) -> Result<()> {
        if let Some(f) = &mut self.trace {
            let ms = platform::now_ms();
            writeln!(
                f,
                "{}",
                serde_json::json!({"time_ms":ms,"direction":direction,"bytes":hex::encode(bytes)})
            )?;
            f.flush()?;
        }
        Ok(())
    }
    /// Wire tracing is diagnostic: a failed trace write must not abort a
    /// transfer mid-sequence (for example after an erase) or drop bytes already
    /// read. Disable the trace once and continue.
    fn trace_wire(&mut self, direction: &str, bytes: &[u8]) {
        if let Err(error) = self.log(direction, bytes) {
            self.trace = None;
            platform::notice(&format!(
                "Wire trace disabled after a write failure: {error:#}"
            ));
        }
    }
    pub(crate) fn normal_pc_slot(&self) -> u8 {
        self.pc_slot
    }
    /// Use a 500 ms write timeout, then restore 100 ms read polling. Neither
    /// transport completion nor a successful flush establishes a device receipt;
    /// callers must still match the operation's protocol response.
    pub fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        let bytes = protocol::encode(control, payload);
        self.trace_wire("tx", &bytes);
        self.port.set_timeout(Duration::from_millis(500))?;
        self.port
            .write_all(&bytes)
            .context("serial write; device outcome may be uncertain")?;
        self.port.flush()?;
        self.port.set_timeout(Duration::from_millis(100))?;
        Ok(())
    }
    pub fn wait(&mut self, timeout: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
        let start = Instant::now();
        loop {
            if let Some(i) = self.incoming.iter().position(&check) {
                return Ok(self.incoming.remove(i).unwrap());
            }
            if start.elapsed() >= timeout {
                bail!("device response timeout after {}ms", timeout.as_millis());
            }
            let mut b = [0u8; 4096];
            match self.port.read(&mut b) {
                Ok(0) => {}
                Ok(n) => {
                    self.trace_wire("rx", &b[..n]);
                    for frame in self.decoder.feed(&b[..n]) {
                        match frame {
                            Ok(f) if fast_idle_frame(&f) => {}
                            Ok(f) => self.incoming.push_back(f),
                            Err(e) => {
                                self.trace_wire("decode-error", e.as_bytes());
                            }
                        }
                    }
                    if self.incoming.len() > 2048 {
                        bail!("unsolicited frame queue overflow");
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e).context("serial read"),
            }
        }
    }
    pub fn exchange(
        &mut self,
        control: u8,
        payload: &[u8],
        timeout: Duration,
        check: impl Fn(&Frame) -> bool,
    ) -> Result<Frame> {
        // Existing queued replies cannot satisfy a new command; asynchronous transfer notifications remain queued.
        self.incoming.retain(|f| !check(f));
        self.send(control, payload)?;
        self.wait(timeout, check)
    }
    fn adapter(&mut self, control: u8, payload: &[u8], response: u8) -> Result<Vec<u8>> {
        Ok(self
            .exchange(control, payload, Duration::from_secs(5), |f| {
                f.control == response
            })?
            .payload)
    }
    fn ack(&mut self, control: u8, payload: &[u8], response: u8) -> Result<()> {
        let p = self.adapter(control, payload, response)?;
        ensure!(
            p.first() == Some(&0),
            "adapter rejected {control:02x}: {}",
            hex::encode(p)
        );
        Ok(())
    }
    /// Reset, unlock the adapter power limit, and establish its supply state.
    /// State 01 means external supply; 02 permits adapter supply; 03/04 mean
    /// charging. The LE16 capacitor threshold 2703 is a raw device quantity,
    /// with no established voltage unit. Supply readiness here says nothing
    /// about traction power or assist readiness.
    ///
    /// The reset, charging, protocol-change and PnP sleeps in this session are
    /// Cranklink's pacing policies, not measured minimum E-TUBE bus timings.
    pub fn initialize(&mut self) -> Result<PowerInfo> {
        // Until this completes, the adapter can be reset without its power
        // setup, so discovery is refused. Routed traffic after a reset must
        // select its target again.
        self.discovery = DiscoveryState::ResetIncomplete;
        self.target_slot = None;
        self.ack(0x10, &[], 0x30)?;
        sleep(Duration::from_secs(3));
        let p = self.adapter(0x1a, &[1], 0x3a)?;
        ensure!(
            p.get(1) == Some(&0),
            "power unlock rejected: {}",
            hex::encode(p)
        );
        let mut state = self.power_state()?;
        if matches!(state, 3 | 4) {
            platform::notice("Charging SM-PCE02 adapter; keep USB connected (up to 90 seconds)...");
            // Poll the supercapacitor every 3 seconds and require a level of
            // at least 2703 before checking power readiness. Charging
            // is automatic after power-limit unlock; do not reset the adapter
            // or enable its discharge while waiting.
            let started = Instant::now();
            let limit = Duration::from_secs(90);
            loop {
                let remaining = limit.saturating_sub(started.elapsed());
                ensure!(
                    !remaining.is_zero(),
                    "SM-PCE02 adapter did not finish charging within 90 seconds (power state 0x{state:02X}); keep USB connected and check its USB power supply before retrying"
                );
                sleep(remaining.min(Duration::from_secs(3)));
                let p = self.adapter(0x1c, &[], 0x3c)?;
                let voltage = p
                    .get(..2)
                    .context("short adapter capacitor voltage reply")?;
                if u16::from_le_bytes([voltage[0], voltage[1]]) < 2703 {
                    continue;
                }
                state = self.power_state()?;
                match state {
                    1 | 2 => break,
                    3 | 4 => continue,
                    _ => bail!("SM-PCE02 cannot supply power while charging (power state 0x{state:02X}); check bike battery power and the E-TUBE connection"),
                }
            }
            platform::notice("SM-PCE02 adapter power is ready; continuing connection...");
        }
        match state {
            1 => self.adapter_supply = false,
            2 => {
                let p = self.adapter(0x1a, &[3], 0x3a)?;
                ensure!(p.get(1) == Some(&0), "adapter supply rejected");
                self.adapter_supply = true;
                sleep(Duration::from_secs(1));
            }
            _ => bail!("unknown adapter power state {state}"),
        }
        sleep(Duration::from_secs(1));
        self.discovery = DiscoveryState::Fresh;
        Ok(PowerInfo {
            adapter_supply: self.adapter_supply,
            raw_state: state,
        })
    }
    fn power_state(&mut self) -> Result<u8> {
        let p = self.adapter(0x1a, &[2], 0x3a)?;
        ensure!(
            p.first() == Some(&2),
            "unexpected power reply: {}",
            hex::encode(&p)
        );
        p.get(1).copied().context("short power reply")
    }
    /// Request master role and confirm it by readback: normal 20 -> A0|slot,
    /// update 60 -> E0|slot. Low five bits identify this PC, not a model.
    pub fn set_protocol(&mut self, protocol: u8) -> Result<()> {
        self.ack(3, &[protocol | 0x20], 0x23)?;
        sleep(Duration::from_secs(1));
        let p = self.adapter(4, &[], 0x24)?;
        let value = *p.first().context("empty protocol reply")?;
        ensure!(
            value & 0xe0 == protocol | 0xa0,
            "wrong protocol reply {}",
            hex::encode(&p)
        );
        self.pc_slot = value & 0x1f;
        Ok(())
    }
    pub fn target(&mut self, slot: u8) -> Result<()> {
        ensure!(slot < 32, "target slot must be0..31");
        self.ack(6, &[slot], 0x26)?;
        self.target_slot = Some(slot);
        Ok(())
    }
    pub fn command(
        &mut self,
        slot: u8,
        group: u8,
        command: u8,
        params: &[u8],
        response_cmd: u8,
    ) -> Result<Vec<u8>> {
        let pc_slot = self.pc_slot;
        let mut p = vec![slot, group, command];
        p.extend_from_slice(params);
        let r = self.exchange(0x48, &p, Duration::from_secs(3), |f| {
            normal_response_matches(f, pc_slot, group, response_cmd)
        })?;
        ensure!(
            r.payload[2] == response_cmd,
            "unit rejected group {group:02X} command {command:02X}: {}",
            hex::encode(&r.payload)
        );
        Ok(r.payload[3..].to_vec())
    }
    /// PnP can reassign slots, and a stop sent afterwards would reach whichever
    /// unit now holds the old slot. Until its Stop is acknowledged, a PC-link
    /// blocks discovery and any further start.
    fn require_no_pc_link(&self) -> Result<()> {
        if let Some(slot) = self.normal_slot {
            bail!("PC-link session at slot {slot} is still active; stop it before discovery or another start");
        }
        Ok(())
    }
    /// A failed `initialize` can leave the adapter reset without its power
    /// setup. Discovery and PC-link start wait until one completes.
    fn require_complete_initialization(&self) -> Result<()> {
        ensure!(
            self.discovery != DiscoveryState::ResetIncomplete,
            "SM-PCE02 initialization did not complete after its reset; initialize the adapter again or reconnect before discovery"
        );
        Ok(())
    }
    pub fn discover(&mut self) -> Result<Vec<u8>> {
        self.require_no_pc_link()?;
        self.require_complete_initialization()?;
        self.discovery = DiscoveryState::Repeated;
        self.set_protocol(0)?;
        let p = self.adapter(0x1d, &[0], 0x3d)?;
        ensure!(p.starts_with(&[0, 0]), "PnP rejected");
        sleep(Duration::from_secs(5));
        let p = self.adapter(0x1b, &[], 0x3b)?;
        ensure!(p.len() >= 4, "short slot bitmap");
        Ok((0..32u8)
            .filter(|&s| p[s as usize / 8] & (1 << (s % 8)) != 0)
            .collect())
    }
    /// On a DU-EP800-CRG, the 16/7C attribute read outside a PC-link was
    /// answered in the first discovery after `initialize`, but not in a later
    /// one on the same connection, while stock identity and a PC-linked 16/7C
    /// still answered. Part queries take the same unlinked path. When discovery
    /// has already run since the last initialization, or an initialization
    /// failed, initialize again first, never while a PC-link is active, so the
    /// next discovery starts from the same reset, power and protocol sequence
    /// as the first. A failure here leaves discovery refused until a later
    /// initialization completes.
    fn reinitialize_for_unlinked_identity(&mut self) -> Result<()> {
        self.require_no_pc_link()?;
        if self.discovery != DiscoveryState::Fresh {
            self.initialize()?;
        }
        Ok(())
    }
    /// No motor PC-link start, key exchange, settings, or boot commands.
    /// A repeated inventory first re-initializes the adapter; see
    /// `reinitialize_for_unlinked_identity`.
    pub fn units(&mut self) -> Result<Vec<UnitIdentity>> {
        self.reinitialize_for_unlinked_identity()?;
        let slots = self.discover()?;
        let mut units = self.inventory_slots(&slots)?;
        for unit in &mut units {
            self.target(unit.slot)?;
            unit.resolve_name(self)?;
        }
        Ok(units)
    }
    /// Stock identities only; omit optional part/model discriminator queries.
    pub fn units_stock_only(&mut self) -> Result<Vec<UnitIdentity>> {
        let slots = self.discover()?;
        self.inventory_slots(&slots)
    }
    fn inventory_slots(&mut self, slots: &[u8]) -> Result<Vec<UnitIdentity>> {
        let mut units = Vec::new();
        for &slot in slots {
            self.target(slot)?;
            let stock = self.command(slot, 1, 0x1c, &[0], 0x1e).with_context(|| {
                format!("stock identity at slot {slot}; inventory stopped without retry")
            })?;
            let mut reply = vec![slot];
            reply.extend(stock);
            units.push(UnitIdentity::from_reply(slot, reply)?);
        }
        Ok(units)
    }
    pub fn probe(&mut self, slot: Option<u8>) -> Result<ProbeIdentity> {
        self.probe_component(slot, None)
    }
    pub fn probe_component(
        &mut self,
        slot: Option<u8>,
        component: Option<&str>,
    ) -> Result<ProbeIdentity> {
        let identity = self.normal_start_component(slot, component)?;
        let serial = self.command(identity.slot, 1, 0x3c, &[0], 0x3e)?;
        let model = UnitIdentity::from_reply(identity.slot, self.model_identity(identity.slot)?)?;
        let serial = if model.is_motor() {
            serial_bytes(&serial)?
        } else {
            component_serial_bytes(&serial)?
        };
        Ok(ProbeIdentity {
            serial: hex::encode(serial),
            model_series: model.model_series,
            model_unit: model.model_unit,
            identity,
        })
    }
    pub fn normal_start(&mut self, slot: Option<u8>) -> Result<Identity> {
        self.normal_start_inner(slot, true, None, false)
    }
    /// Start PC linking at a motor selected from the current discovery map.
    /// The caller must not run PnP between selecting its identity and this call:
    /// another discovery can reassign slots on a multi-component bike.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn normal_start_discovered(&mut self, slot: u8) -> Result<Identity> {
        self.normal_start_selected(slot, true, None, false)
    }
    pub fn normal_start_component(
        &mut self,
        slot: Option<u8>,
        component: Option<&str>,
    ) -> Result<Identity> {
        self.normal_start_inner(slot, true, component, slot.is_some() || component.is_some())
    }
    /// Explicit RX recovery entry: DCAS must be alive and identifiable, while
    /// the interrupted RX application can make both application version reads
    /// unavailable. This entry requires cargo; its caller must independently
    /// validate the DCAS bootloader identity before any firmware write.
    /// Ordinary motor probe/verification requires both application versions.
    pub fn normal_start_for_rx_recovery(&mut self, slot: Option<u8>) -> Result<Identity> {
        self.normal_start_inner(slot, false, None, false)
    }
    fn normal_start_inner(
        &mut self,
        slot: Option<u8>,
        require_versions: bool,
        component: Option<&str>,
        allow_component: bool,
    ) -> Result<Identity> {
        // Explicit selections can refine identities before the PC-link starts.
        if allow_component || component.is_some() {
            self.reinitialize_for_unlinked_identity()?;
        }
        let slots = self.discover()?;
        let slot = if let Some(component) = component {
            let named_slots = match slot {
                Some(slot) => vec![select_motor_slot(&slots, Some(slot))?],
                None => slots.clone(),
            };
            let mut units = self.inventory_slots(&named_slots)?;
            for unit in &mut units {
                if unit
                    .model_candidates
                    .iter()
                    .any(|name| crate::unit_models::name_matches(component, name))
                {
                    self.target(unit.slot)?;
                    unit.resolve_name(self)?;
                }
            }
            select_named_component(&units, component)?
        } else {
            match slot {
                Some(slot) => select_motor_slot(&slots, Some(slot))?,
                None if slots.len() == 1 => slots[0],
                None => select_current_motor(&self.inventory_slots(&slots)?)?,
            }
        };
        self.normal_start_selected(slot, require_versions, component, allow_component)
    }
    /// Start 32/10 with the current PC source, send all five ordered 32/30 keys,
    /// and require 32/12 start receipt before further service queries. The keys
    /// establish PC-link, independently of protected-setting authorization.
    /// Wait 300 ms after start on adapter supply, otherwise 1 s, then 100 ms
    /// between keys. Repeating start is not a keepalive for an active session.
    fn normal_start_selected(
        &mut self,
        slot: u8,
        require_versions: bool,
        component: Option<&str>,
        allow_component: bool,
    ) -> Result<Identity> {
        self.require_no_pc_link()?;
        self.require_complete_initialization()?;
        self.target(slot)?;
        let mut unit = UnitIdentity::from_reply(slot, self.model_identity(slot)?)?;
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
        let p = self.adapter(4, &[], 0x24)?;
        self.pc_slot = p.first().copied().context("short protocol reply")? & 0x1f;
        self.normal_slot = Some(slot);
        self.normal_is_motor = unit.is_motor();
        self.send(0x48, &[slot, 0x32, 0x10, 1, self.pc_slot, 0, 0])?;
        sleep(Duration::from_millis(if self.adapter_supply {
            300
        } else {
            1000
        }));
        for (i, key) in [
            [0xa2, 0x2b],
            [0x30, 0x0e],
            [0x7a, 0x4d],
            [0x62, 0x2b],
            [0x85, 0xb4],
        ]
        .iter()
        .enumerate()
        {
            if i > 0 {
                sleep(Duration::from_millis(100));
            }
            self.send(0x48, &[slot, 0x32, 0x30, key[0], key[1], 0, 0])?;
        }
        let pc_slot = self.pc_slot;
        self.wait(Duration::from_secs(3), |f| {
            f.control == 0x48 && f.payload.starts_with(&[pc_slot, 0x32, 0x12, 1])
        })?;
        self.normal_slot = Some(slot);
        sleep(Duration::from_millis(30));
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
            matches!(unit.model_series, 0x08 | 0x0a) || cargo.len() >= 7,
            "truncated cargo attribute reply: {}",
            hex::encode(&cargo)
        );
        let dcas_version = if require_versions {
            version(&self.command(slot, 1, 0x2c, &[0], 0x2e)?)?
        } else {
            "unavailable (RX recovery)".to_owned()
        };
        let rx_version = if require_versions {
            version(&self.command(slot, 1, 0x84, &[1], 0x86)?)?
        } else {
            "unavailable (RX recovery)".to_owned()
        };
        Ok(Identity {
            slot,
            cargo: String::from_utf8_lossy(&cargo)
                .trim_end_matches('\0')
                .into(),
            dcas_version,
            rx_version,
        })
    }
    /// One regulation-auth attempt in the existing PC-link session.
    /// The serial, live model and firmware capabilities are checked before any key.
    pub fn authenticate_unit_settings(
        &mut self,
        expected_serial: &str,
    ) -> Result<serde_json::Value> {
        use crate::ble::unit_auth;
        ensure!(
            !self.unit_auth_attempted,
            "drive-unit authentication already attempted; no automatic retry"
        );
        let slot = self
            .normal_slot
            .context("start the motor PC-link session first")?;
        let serial = serial_bytes(&self.command(slot, 1, 0x3c, &[0], 0x3e)?)?;
        ensure!(
            hex::encode(serial).eq_ignore_ascii_case(expected_serial),
            "drive-unit authentication motor serial mismatch"
        );
        let model = self.model_identity(slot)?;
        ensure!(model.len() >= 3, "invalid motor stock identity");
        UnitIdentity::from_reply(slot, model.clone())?.require_motor()?;
        let cargo = crate::motor_support::read_params(self, slot, model[1])?;
        let firmware = version(&self.command(slot, 1, 0x2c, &[0], 0x2e)?)?;
        ensure!(model.len() >= 3, "invalid motor stock identity");
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
        // Disable physical TX/RX tracing for the entire exchange. On failure,
        // keep it disabled during cleanup: delayed fragments can contain keys.
        self.log("authentication-redacted", &[])?;
        let trace = self.trace.take();
        let result = (|| -> Result<()> {
            self.incoming.clear();
            if let Some(product) = product.as_deref() {
                return self.authenticate_modern_settings(slot, &serial, product);
            }
            let keys = unit_auth::derive(&serial);
            let mut request = zeroize::Zeroizing::new(vec![slot, 0x16, 0xd8]);
            request.extend_from_slice(&keys.auth_key);
            self.send(0x48, &request)?;
            let pc = self.pc_slot;
            let mut challenge = zeroize::Zeroizing::new([0u8; 16]);
            for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
                let reply = self.wait(Duration::from_secs(3), |f| {
                    normal_response_matches(f, pc, 0x16, 0xda)
                })?;
                ensure!(reply.payload.len() == 10 && reply.payload[2..4] == [0xda, selector],
                    "drive-unit authentication challenge rejected, malformed or out of order; no retry");
                challenge[offset..offset + length].copy_from_slice(&reply.payload[4..4 + length]);
            }
            let proof = zeroize::Zeroizing::new(unit_auth::encrypt_challenge(&keys, &challenge));
            for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
                let mut request = zeroize::Zeroizing::new([0xff; 10]);
                request[..4].copy_from_slice(&[slot, 0x16, 0xe0, selector]);
                request[4..4 + length].copy_from_slice(&proof[offset..offset + length]);
                self.send(0x48, request.as_ref())?;
                sleep(Duration::from_millis(100));
            }
            let reply = self.wait(Duration::from_secs(3), |f| {
                normal_response_matches(f, pc, 0x16, 0xe2)
            })?;
            unit_auth::validate_receipt(&reply.payload, pc, 0x16, 0xe2)?;
            Ok(())
        })();
        if result.is_ok() {
            self.trace = trace;
            self.log("authentication-verified", &[])?;
        }
        result?;
        Ok(
            serde_json::json!({"unit_authentication_receipt":true,"firmware":firmware,
            "protocol":if modern {"modern"} else {"legacy"},
            "scope":"regulation-setting authentication; per-setting validation still required"}),
        )
    }
    fn authenticate_modern_settings(
        &mut self,
        slot: u8,
        serial: &[u8; 6],
        product: &str,
    ) -> Result<()> {
        use crate::ble::unit_auth;
        let keys = unit_auth::derive_modern(serial, product)?;
        let ciphertext = zeroize::Zeroizing::new(keys.auth_ciphertext());
        self.send_modern_auth_fragments(slot, 0x68, &ciphertext)?;
        let pc = self.pc_slot;
        let mut challenge = zeroize::Zeroizing::new([0u8; 16]);
        for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
            let reply = self.wait(Duration::from_secs(3), |f| {
                normal_response_matches(f, pc, 0x33, 0x6a)
            })?;
            ensure!(
                reply.payload.len() == 10 && reply.payload[2..4] == [0x6a, selector],
                "modern drive-unit authentication challenge rejected, malformed or out of order; no retry"
            );
            challenge[offset..offset + length].copy_from_slice(&reply.payload[4..4 + length]);
        }
        let proof = zeroize::Zeroizing::new(keys.challenge_proof(&challenge));
        self.send_modern_auth_fragments(slot, 0x70, &proof)?;
        let reply = self.wait(Duration::from_secs(3), |f| {
            normal_response_matches(f, pc, 0x33, 0x72)
        })?;
        unit_auth::validate_receipt(&reply.payload, pc, 0x33, 0x72)
    }
    fn send_modern_auth_fragments(
        &mut self,
        slot: u8,
        command: u8,
        block: &[u8; 16],
    ) -> Result<()> {
        for (selector, offset, length) in [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)] {
            let mut request = zeroize::Zeroizing::new([0xff; 10]);
            request[..4].copy_from_slice(&[slot, 0x33, command, selector]);
            request[4..4 + length].copy_from_slice(&block[offset..offset + length]);
            self.send(0x48, request.as_ref())?;
            sleep(Duration::from_millis(100));
        }
        Ok(())
    }
    /// Stop with the same PC source and require 32/12 parameter 00 before
    /// releasing local session state. A transport close alone proves no stop.
    pub fn normal_stop(&mut self) -> Result<()> {
        if let Some(slot) = self.normal_slot {
            let reply = self.command(slot, 0x32, 0x10, &[0, self.pc_slot, 0, 0], 0x12)?;
            ensure!(
                reply.first() == Some(&0),
                "PC-link Stop mode not acknowledged: {}",
                hex::encode(reply)
            );
            self.normal_slot = None;
            self.normal_is_motor = false;
        }
        Ok(())
    }
    /// Prepare the single discovered DU for the SM-PCE02 2MHz boot bridge.
    /// Caller first sends application PCA_UPDATE_SET. Slot assignment remains
    /// the discovered slot; this implementation handles one motor exclusively.
    pub fn fast_prepare(&mut self, slot: u8) -> Result<u8> {
        ensure!(
            self.normal_slot == Some(slot) && self.normal_is_motor,
            "fast preparation requires the active discovered motor"
        );
        self.set_protocol(0x40)?;
        self.target(slot)?;
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
            } else {
                self.exchange(0x88, &p, Duration::from_secs(3), |f| {
                    f.control & 0x88 == 0x88 && f.payload.starts_with(&[0x11, (i + 1) as u8])
                })?;
            }
        }
        self.normal_slot = None;
        self.normal_is_motor = false;
        // DCASX-specific second pass in sendStartUpdateForConnectedUnits:
        // wait at least 1000 ms after key5 dispatch, then repeat key5 and require
        // its bootloader acknowledgment at the explicitly retained slot.
        sleep(Duration::from_secs(1));
        self.target(slot)?;
        self.exchange(0x88, &[5, 0x33, 0x83, 0x27], Duration::from_secs(3), |f| {
            f.control & 0x88 == 0x88 && f.payload.starts_with(&[0x11, 5])
        })?;
        self.fast_adapter_slots(0, 1)?;
        self.fast_adapter_mode(5)?;
        self.target(slot)?;
        // First 4B is sent at 1 MHz without waiting; the acknowledgment is
        // received at 2 MHz after the adapter switches its own bus modulation.
        self.send(0x88, &[0x4b, 0, 0, 0])?;
        sleep(Duration::from_millis(100));
        self.fast_adapter_mode(6)?;
        self.wait(Duration::from_secs(3), |f| {
            f.control == 0xa8 && f.payload.len() == 16 && f.payload.starts_with(&[slot, 0x31])
        })?;
        self.fast_boot_command(slot, 0x4c, 0xff, 0x55)?;
        self.fast_adapter_slots(0, !(1u32 << slot))?;
        self.fast_adapter_slots(1, 1u32 << slot)?;
        sleep(Duration::from_millis(100));
        self.fast_boot_command(slot, 0x4c, slot, 0x55)?;
        let board = self.fast_boot_command(slot, 0x2f, 0, 0x3b)?;
        ensure!(
            board.payload.len() >= 4 && crate::firmware::is_allowed_motor_board(board.payload[3]),
            "fast bridge board mismatch: {}",
            hex::encode(&board.payload)
        );
        Ok(slot)
    }
    fn fast_adapter_slots(&mut self, command: u8, mask: u32) -> Result<()> {
        let mut p = vec![command];
        p.extend_from_slice(&mask.to_le_bytes());
        let r = self.adapter(7, &p, 0x27)?;
        ensure!(
            r.starts_with(&[command, 0]),
            "adapter slot/mode request rejected: {}",
            hex::encode(r)
        );
        Ok(())
    }
    fn fast_adapter_mode(&mut self, mode: u8) -> Result<()> {
        let r = self.adapter(7, &[mode], 0x27)?;
        ensure!(
            r.starts_with(&[mode, 0]),
            "adapter communication-mode request rejected: {}",
            hex::encode(r)
        );
        Ok(())
    }
    fn fast_boot_command(
        &mut self,
        slot: u8,
        command: u8,
        param: u8,
        response: u8,
    ) -> Result<Frame> {
        let mut p = [0u8; 16];
        p[..3].copy_from_slice(&[slot, command, param]);
        self.exchange(0xa8, &p, Duration::from_secs(3), |f| {
            fast_boot_response_matches(f, slot, command, param, response)
        })
    }
    /// Return the unchanged DCAS image from its2MHz boot bridge to application.
    pub fn fast_finish(&mut self, slot: u8) -> Result<()> {
        self.fast_boot_command(slot, 0x4c, slot, 0x55)?;
        self.fast_boot_command(slot, 0x28, 0, 0x31)?;
        self.fast_adapter_slots(0, 1)?;
        self.fast_adapter_mode(5)?;
        sleep(Duration::from_secs(1));
        Ok(())
    }
    /// Enter legacy recovery using the five firmware keys. Application transition
    /// retains its discovered slot before switching to the update protocol.
    pub fn boot_start(&mut self, from_application: bool) -> Result<BootIdentity> {
        self.boot_start_at(from_application, None)
    }
    /// Application entry first sends 01/C0 [slot] -> C2 to retain its address;
    /// recovery uses the explicit slot or 31. Then select UPDATE/master (60),
    /// restore the target, and send the five Pi key records over control 88.
    /// Each receipt is 11,key-index, distinct from ordinary service +2 replies.
    ///
    /// On application handoff, dispatch key 5, restore protocol/target after the
    /// transition, and repeat key 5 for its boot receipt. Recovery sends it once.
    /// These keys and subsequent legacy boot queries have no payload slot byte:
    /// correlation depends on exclusive selected-target state and serialized
    /// requests, not on a universally addressed 88 reply.
    pub fn boot_start_at(
        &mut self,
        from_application: bool,
        recovery_slot: Option<u8>,
    ) -> Result<BootIdentity> {
        const KEYS: [[u8; 3]; 5] = [
            [0x31, 0x41, 0x59],
            [0x26, 0x53, 0x58],
            [0x97, 0x93, 0x23],
            [0x84, 0x62, 0x64],
            [0x33, 0x83, 0x27],
        ];
        let initial_slot = if from_application {
            ensure!(
                self.normal_is_motor,
                "application bootloader entry requires an active motor session"
            );
            self.normal_slot
                .context("application transition requires a discovered normal session")?
        } else {
            recovery_slot.unwrap_or(31)
        };
        ensure!(initial_slot < 32, "recovery slot must be0..31");
        if from_application {
            self.command(initial_slot, 1, 0xc0, &[initial_slot], 0xc2)
                .context("retain discovered slot across bootloader entry")?;
        }
        self.set_protocol(0x40)?;
        self.target(initial_slot)?;
        for (i, key) in KEYS.iter().enumerate() {
            let p = [(i + 1) as u8, key[0], key[1], key[2]];
            if i == 4 && from_application {
                self.send(0x88, &p)?;
                sleep(Duration::from_millis(100));
                self.set_protocol(0x40)?;
                sleep(Duration::from_secs(1));
                self.target(initial_slot)?;
            }
            self.exchange(0x88, &p, Duration::from_secs(5), |f| {
                f.control & 0x88 == 0x88 && f.payload.starts_with(&[0x11, (i + 1) as u8])
            })?;
        }
        self.normal_slot = None;
        self.normal_is_motor = false;
        self.boot_identity()
    }
    fn boot_query(
        &mut self,
        payload: &[u8],
        response: u8,
        min: usize,
        timeout: u64,
    ) -> Result<Vec<u8>> {
        let f = self.exchange(0x88, payload, Duration::from_millis(timeout), |f| {
            f.control & 0x88 == 0x88
                && f.payload.first() == Some(&response)
                && f.payload.len() >= min
        })?;
        Ok(f.payload)
    }
    pub fn boot_reset(&mut self) -> Result<()> {
        self.boot_query(&[0x28, 0, 0, 0], 0x31, 1, 3000)?;
        Ok(())
    }
    pub fn boot_identity(&mut self) -> Result<BootIdentity> {
        let lo = self.boot_query(&[0x2e, 0, 0, 0], 0x3a, 3, 3000)?;
        let hi = self.boot_query(&[0x2f, 0, 0, 0], 0x3b, 3, 3000)?;
        let unit = self.boot_query(&[0x30, 0, 0, 0], 0x3c, 3, 3000)?;
        let a = self.boot_query(&[0x29, 0, 0, 0], 0x35, 4, 3000)?;
        let b = self.boot_query(&[0x2a, 0, 0, 0], 0x36, 4, 3000)?;
        Ok(BootIdentity {
            slot: self
                .target_slot
                .context("bootloader target address has not been selected")?,
            version: [lo[1] >> 4, lo[1] & 15, lo[2], hi[1]],
            board: hi[2],
            series: unit[1],
            unit: unit[2],
            serial: [a[1], a[2], a[3], b[1], b[2], b[3]],
        })
    }
    /// Restore the legacy DCASX image. Each buffer dispatch, physical transfer
    /// completion, and motor checksum is independently acknowledged. Never retries
    /// erase/program after uncertain outcomes.
    pub fn flash_dcas(
        &mut self,
        image: &[u8],
        identity: &BootIdentity,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<FlashReceipt> {
        crate::firmware_io::flash_dcas(self, image, identity, &mut progress)
    }
}
fn fast_idle_frame(f: &Frame) -> bool {
    (f.control == 0x88 && f.payload == [0x70, 0, 0, 0x78])
        || (f.control == 0xa8
            && f.payload.len() == 16
            && f.payload[0] < 32
            && f.payload[1] == 0x70
            && f.payload[2..].iter().all(|&b| b == 0))
}
fn fast_boot_response_matches(f: &Frame, slot: u8, command: u8, param: u8, response: u8) -> bool {
    f.control == 0xa8
        && f.payload.starts_with(&[slot, response])
        && (command != 0x4c || f.payload.get(2) == Some(&param))
}
/// PnP can permute component addresses without changing the occupied bitmap.
/// Resolve by stock identity in this discovery session, never a previous map.
pub(crate) fn select_current_motor(units: &[UnitIdentity]) -> Result<u8> {
    let motors: Vec<_> = units
        .iter()
        .filter(|unit| unit.is_motor())
        .map(|unit| unit.slot)
        .collect();
    ensure!(motors.len() == 1, "expected one recognized motor in current discovery; found motor slots {motors:?}; select --slot explicitly when multiple motors are present");
    Ok(motors[0])
}
pub(crate) fn select_named_component(units: &[UnitIdentity], selector: &str) -> Result<u8> {
    let matching: Vec<_> = units
        .iter()
        .filter(|unit| {
            unit.model
                .is_some_and(|model| crate::unit_models::name_matches(selector, model))
        })
        .map(|unit| unit.slot)
        .collect();
    ensure!(matching.len() == 1, "component {selector:?} must resolve to exactly one connected unit; matching slots {matching:?}; inspect units for unresolved or duplicate names");
    Ok(matching[0])
}
pub(crate) fn select_motor_slot(slots: &[u8], selected: Option<u8>) -> Result<u8> {
    if let Some(slot) = selected {
        ensure!(slots.contains(&slot), "slot {slot} absent; found {slots:?}");
        return Ok(slot);
    }
    ensure!(
        slots.len() == 1,
        "select --slot from discovered slots {slots:?}; units lists component identities"
    );
    Ok(slots[0])
}
fn normal_response_matches(f: &Frame, pc_slot: u8, group: u8, response_cmd: u8) -> bool {
    f.control == 0x48
        && f.payload.len() >= 3
        && f.payload[0] == pc_slot
        && f.payload[1] == group
        && (f.payload[2] == response_cmd || f.payload[2] == response_cmd.wrapping_add(1))
}
fn version(p: &[u8]) -> Result<String> {
    ensure!(p.len() >= 3, "short version reply");
    Ok(format!("{}.{}.{}.{}", p[0] >> 4, p[0] & 15, p[1], p[2]))
}

pub(crate) fn unindexed_dcas_ack(f: &Frame) -> bool {
    f.control & 0x88 == 0x88 && f.payload.starts_with(&[0x31, 0, 0])
}
pub(crate) fn dcas_buffer_packet(packet: u8, index: u16, block: &[u8; 64], sum: u8) -> Vec<u8> {
    let mut p = vec![packet, 2, 0];
    p.extend_from_slice(block);
    p.push(sum);
    p.extend_from_slice(&index.to_le_bytes());
    p
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    struct ScriptPort {
        steps: std::rc::Rc<std::cell::RefCell<VecDeque<(Frame, Frame)>>>,
        pending: VecDeque<u8>,
    }
    impl Read for ScriptPort {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let n = bytes.len().min(self.pending.len());
            for byte in &mut bytes[..n] {
                *byte = self.pending.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    impl Write for ScriptPort {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let frames = Decoder::default().feed(bytes);
            assert_eq!(frames.len(), 1);
            let actual = frames.into_iter().next().unwrap().unwrap();
            let (expected, response) = self
                .steps
                .borrow_mut()
                .pop_front()
                .expect("unexpected wire command");
            assert_eq!(actual.control, expected.control);
            assert_eq!(actual.payload, expected.payload);
            self.pending
                .extend(protocol::encode(response.control, &response.payload));
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Port for ScriptPort {
        fn set_timeout(&mut self, _: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }
    pub(crate) type ScriptSteps = std::rc::Rc<std::cell::RefCell<VecDeque<(Frame, Frame)>>>;
    /// A freshly opened session over `port`, as `Session::open` leaves it.
    fn session_over(port: Box<dyn Port>) -> Session {
        Session {
            port,
            decoder: Decoder::default(),
            incoming: VecDeque::new(),
            trace: None,
            pc_slot: 0,
            normal_slot: None,
            normal_is_motor: false,
            target_slot: None,
            adapter_supply: false,
            unit_auth_attempted: false,
            discovery: DiscoveryState::Fresh,
        }
    }
    /// A session over a port that asserts each request and answers it in order.
    pub(crate) fn scripted(steps: Vec<(Frame, Frame)>) -> (Session, ScriptSteps) {
        let steps = std::rc::Rc::new(std::cell::RefCell::new(steps.into()));
        let session = session_over(Box::new(ScriptPort {
            steps: steps.clone(),
            pending: VecDeque::new(),
        }));
        (session, steps)
    }
    const PC_LINK_KEYS: [[u8; 2]; 5] = [
        [0xa2, 0x2b],
        [0x30, 0x0e],
        [0x7a, 0x4d],
        [0x62, 0x2b],
        [0x85, 0xb4],
    ];
    /// A stateful SM-PCE02 with a DU-EP800-CRG at slot 13. It models the
    /// hardware report against `units` in one `cranklink session` connection:
    /// the motor answers stock reads at any time and the 16/7C attribute read
    /// inside a PC-link, but outside one only during the first discovery after
    /// an adapter reset. A repeated PnP without a reset, as an earlier `units`
    /// or `probe` leaves it, gets stock replies and no 16/7C reply. Hardware
    /// has not confirmed which earlier step closes that window; the model only
    /// reproduces the observed behaviour. Unmodelled requests panic.
    #[derive(Default)]
    pub(crate) struct ModeledBus {
        master: bool,
        target: Option<u8>,
        discoveries_since_reset: u32,
        next_key: Option<usize>,
        linked: bool,
        pub(crate) resets: u32,
        pub(crate) unanswered_attribute_reads: u32,
        /// Reject this many power unlocks after their reset.
        pub(crate) rejected_power_unlocks: u32,
        pub(crate) requests: u32,
    }
    impl ModeledBus {
        pub(crate) const MOTOR: u8 = 13;
        pub(crate) fn respond(&mut self, request: &Frame) -> Vec<Frame> {
            let reply = |control: u8, payload: &[u8]| vec![frame(control, payload)];
            self.requests += 1;
            match (request.control, request.payload.as_slice()) {
                (0x10, []) => {
                    assert!(!self.linked, "adapter reset during an active PC-link");
                    self.resets += 1;
                    self.master = false;
                    self.target = None;
                    self.discoveries_since_reset = 0;
                    reply(0x30, &[0])
                }
                (0x1a, [1]) if self.rejected_power_unlocks > 0 => {
                    self.rejected_power_unlocks -= 1;
                    reply(0x3a, &[1, 1])
                }
                (0x1a, [1]) => reply(0x3a, &[1, 0]),
                (0x1a, [2]) => reply(0x3a, &[2, 1]),
                (3, [0x20]) => {
                    // A role request invalidates the target selection.
                    self.master = true;
                    self.target = None;
                    reply(0x23, &[0])
                }
                (4, []) => reply(0x24, &[if self.master { 0xa0 } else { 0x80 }]),
                (0x1d, [0]) if self.master => {
                    self.discoveries_since_reset += 1;
                    reply(0x3d, &[0, 0])
                }
                (0x1b, []) if self.master => reply(0x3b, &[0, 1 << (Self::MOTOR - 8), 0, 0]),
                (6, [slot]) => {
                    self.target = Some(*slot);
                    reply(0x26, &[0])
                }
                (0x48, [slot, request @ ..])
                    if *slot == Self::MOTOR && self.master && self.target == Some(*slot) =>
                {
                    self.motor(request)
                }
                _ => panic!("unmodelled adapter request {request:02x?}"),
            }
        }
        fn motor(&mut self, request: &[u8]) -> Vec<Frame> {
            let reply = |payload: &[u8]| vec![frame(0x48, &[&[0], payload].concat())];
            match request {
                [1, 0x1c, 0] => reply(&[1, 0x1e, 0x23, 0]),
                [0x16, 0x7c, 0] if self.linked || self.discoveries_since_reset == 1 => {
                    reply(&[0x16, 0x7e, b'0', b'0', b'-', b'C', b'R', b'G', 0])
                }
                [0x16, 0x7c, 0] => {
                    self.unanswered_attribute_reads += 1;
                    Vec::new()
                }
                [0x32, 0x10, 1, 0, 0, 0] if !self.linked => {
                    self.next_key = Some(0);
                    Vec::new()
                }
                [0x32, 0x30, a, b, 0, 0] => {
                    let index = self.next_key.take().expect("PC-link key before start");
                    assert_eq!([*a, *b], PC_LINK_KEYS[index], "PC-link key {index}");
                    if index + 1 < PC_LINK_KEYS.len() {
                        self.next_key = Some(index + 1);
                        return Vec::new();
                    }
                    self.linked = true;
                    reply(&[0x32, 0x12, 1, 0, 0xff, 0xff])
                }
                [0x32, 0x10, 0, 0, 0, 0] if self.linked => {
                    self.linked = false;
                    reply(&[0x32, 0x12, 0])
                }
                [1, 0x2c, 0] if self.linked => reply(&[1, 0x2e, 0x42, 0, 0]),
                [1, 0x84, 1] if self.linked => reply(&[1, 0x86, 0x41, 11, 0]),
                [1, 0x3c, 0] if self.linked => reply(&[1, 0x3e, 1, 2, 3, 4, 5, 6, 0xff]),
                _ => panic!("unmodelled motor request {request:02x?}"),
            }
        }
    }
    struct ModelPort {
        bus: std::rc::Rc<std::cell::RefCell<ModeledBus>>,
        decoder: Decoder,
        pending: VecDeque<u8>,
    }
    impl Read for ModelPort {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let n = bytes.len().min(self.pending.len());
            for byte in &mut bytes[..n] {
                *byte = self.pending.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    impl Write for ModelPort {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            for request in self.decoder.feed(bytes) {
                for reply in self.bus.borrow_mut().respond(&request.unwrap()) {
                    self.pending
                        .extend(protocol::encode(reply.control, &reply.payload));
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Port for ModelPort {
        fn set_timeout(&mut self, _: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }
    /// A freshly opened session over `ModeledBus`.
    fn modeled() -> (Session, std::rc::Rc<std::cell::RefCell<ModeledBus>>) {
        let bus = std::rc::Rc::default();
        let session = session_over(Box::new(ModelPort {
            bus: std::rc::Rc::clone(&bus),
            decoder: Decoder::default(),
            pending: VecDeque::new(),
        }));
        (session, bus)
    }
    /// A scripted session whose PC-link to the motor at `slot` is already open.
    pub(crate) fn scripted_motor(slot: u8, steps: Vec<(Frame, Frame)>) -> (Session, ScriptSteps) {
        let (mut session, steps) = scripted(steps);
        session.normal_slot = Some(slot);
        session.normal_is_motor = true;
        (session, steps)
    }
    pub(crate) fn frame(control: u8, payload: &[u8]) -> Frame {
        Frame {
            control,
            payload: payload.to_vec(),
        }
    }
    fn discovery_steps(slots: &[u8]) -> Vec<(Frame, Frame)> {
        let mut bitmap = [0; 4];
        for &slot in slots {
            bitmap[usize::from(slot) / 8] |= 1 << (slot % 8);
        }
        vec![
            (frame(3, &[0x20]), frame(0x23, &[0])),
            (frame(4, &[]), frame(0x24, &[0xa0])),
            (frame(0x1d, &[0]), frame(0x3d, &[0, 0])),
            (frame(0x1b, &[]), frame(0x3b, &bitmap)),
        ]
    }
    fn stock_step(slot: u8, series: u8, unit: u8) -> (Frame, Frame) {
        (
            frame(0x48, &[slot, 1, 0x1c, 0]),
            frame(0x48, &[0, 1, 0x1e, series, unit]),
        )
    }
    #[test]
    fn modeled_bus_withholds_unlinked_attributes_after_a_repeated_discovery() {
        let (mut session, bus) = modeled();
        session.initialize().unwrap();
        let attributes = |session: &mut Session| {
            session.target(ModeledBus::MOTOR).unwrap();
            let request = [ModeledBus::MOTOR, 0x16, 0x7c, 0];
            session.exchange(0x48, &request, Duration::from_millis(100), |f| {
                normal_response_matches(f, 0, 0x16, 0x7e)
            })
        };
        session.discover().unwrap();
        attributes(&mut session).unwrap();
        // The hardware symptom: stock identity still answers, 16/7C does not.
        assert_eq!(session.discover().unwrap(), [ModeledBus::MOTOR]);
        session.target(ModeledBus::MOTOR).unwrap();
        let stock = session.command(ModeledBus::MOTOR, 1, 0x1c, &[0], 0x1e);
        assert_eq!(stock.unwrap(), [0x23, 0]);
        assert!(attributes(&mut session).is_err());
        assert_eq!(bus.borrow().unanswered_attribute_reads, 1);
    }
    #[test]
    fn repeated_units_and_probes_share_one_connection() {
        let (mut session, bus) = modeled();
        session.initialize().unwrap();
        let resolved = |units: Vec<UnitIdentity>| -> Vec<(u8, Option<&str>)> {
            units.iter().map(|unit| (unit.slot, unit.model)).collect()
        };
        let motor = [(ModeledBus::MOTOR, Some("DU-EP800-CRG"))];
        // The first inventory after initialization adds no reset, as in
        // `cranklink units`; a repeated one starts from a new initialization.
        assert_eq!(resolved(session.units().unwrap()), motor);
        assert_eq!(bus.borrow().resets, 1);
        assert_eq!(resolved(session.units().unwrap()), motor);
        assert_eq!(bus.borrow().resets, 2);
        // A default probe reads 16/7C inside its PC-link and needs no reset.
        let probe = session.probe(None).unwrap();
        session.normal_stop().unwrap();
        assert_eq!(probe.identity.cargo, "00-CRG");
        assert_eq!(probe.serial, "010203040506");
        assert_eq!(bus.borrow().resets, 2);
        assert_eq!(resolved(session.units().unwrap()), motor);
        assert_eq!(bus.borrow().resets, 3);
        // A named probe resolves 16/7C before its PC-link, like an inventory.
        let named = session.probe_component(None, Some("DU-EP800-CRG")).unwrap();
        session.normal_stop().unwrap();
        assert_eq!(named.identity.slot, ModeledBus::MOTOR);
        assert_eq!(bus.borrow().resets, 4);
        // An active PC-link refuses both before any reset reaches the adapter.
        session.normal_start(None).unwrap();
        let error = session.units().unwrap_err().to_string();
        assert!(error.contains("still active"), "{error}");
        assert!(session.probe_component(None, Some("DU-EP800-CRG")).is_err());
        session.normal_stop().unwrap();
        assert_eq!(bus.borrow().resets, 4);
        assert_eq!(bus.borrow().unanswered_attribute_reads, 0);
    }
    #[test]
    fn failed_reinitialization_refuses_discovery_until_one_completes() {
        let (mut session, bus) = modeled();
        session.initialize().unwrap();
        session.units().unwrap();
        // The repeated inventory resets the adapter, then its power setup fails.
        bus.borrow_mut().rejected_power_unlocks = 1;
        let error = session.units().unwrap_err().to_string();
        assert!(error.contains("power unlock rejected"), "{error}");
        assert_eq!(bus.borrow().resets, 2);
        // A default probe, which does not initialize, must not discover or
        // start a PC-link on the half-initialized adapter.
        let requests = bus.borrow().requests;
        let error = session.probe(None).unwrap_err().to_string();
        assert!(error.contains("did not complete"), "{error}");
        assert_eq!(bus.borrow().requests, requests);
        // An inventory initializes again; then probes work as before.
        let units = session.units().unwrap();
        assert_eq!(units[0].model, Some("DU-EP800-CRG"));
        assert_eq!(bus.borrow().resets, 3);
        session.probe(None).unwrap();
        session.normal_stop().unwrap();
        assert_eq!(bus.borrow().resets, 3);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn failed_trace_writes_disable_the_trace_without_failing_the_exchange() {
        let (mut session, steps) = scripted(vec![(frame(4, &[]), frame(0x24, &[0xa0]))]);
        // Every write to /dev/full fails with ENOSPC.
        session.trace = Some(OpenOptions::new().append(true).open("/dev/full").unwrap());
        session.send(4, &[]).unwrap();
        assert!(session.trace.is_none());
        let reply = session
            .wait(Duration::from_secs(1), |f| f.control == 0x24)
            .unwrap();
        assert_eq!(reply.payload, [0xa0]);
        assert!(steps.borrow().is_empty());
    }
    fn component_start_steps(slot: u8) -> Vec<(Frame, Frame)> {
        let mut steps = vec![
            (frame(4, &[]), frame(0x24, &[0xa0])),
            (
                frame(0x48, &[slot, 0x32, 0x10, 1, 0, 0, 0]),
                frame(0x48, &[0, 0x32, 0x12, 1]),
            ),
        ];
        for [a, b] in [
            [0xa2, 0x2b],
            [0x30, 0x0e],
            [0x7a, 0x4d],
            [0x62, 0x2b],
            [0x85, 0xb4],
        ] {
            steps.push((
                frame(0x48, &[slot, 0x32, 0x30, a, b, 0, 0]),
                frame(0x48, &[0]),
            ));
        }
        steps.push((
            frame(0x48, &[slot, 1, 0x2c, 0]),
            frame(0x48, &[0, 1, 0x2e, 0x31, 2, 3]),
        ));
        steps
    }
    fn check_component_slot_start(series: u8, unit: u8, model: &str) {
        let mut commands = discovery_steps(&[4]);
        commands.push((frame(6, &[4]), frame(0x26, &[0])));
        commands.push(stock_step(4, series, unit));
        commands.extend(component_start_steps(4));
        let (mut session, steps) = scripted(commands);
        let identity = session.normal_start_component(Some(4), None).unwrap();
        assert_eq!(identity.cargo, model);
        assert_eq!(identity.dcas_version, "3.1.2.3");
        assert_eq!(identity.rx_version, "not applicable");
        assert_eq!(session.normal_slot, Some(4));
        assert!(!session.normal_is_motor);
        assert!(session
            .boot_start(true)
            .unwrap_err()
            .to_string()
            .contains("requires an active motor session"));
        assert!(session.fast_prepare(4).is_err());
        // The script admits no motor discriminator or RX-version requests.
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn explicit_derailleur_session_uses_generic_handshake_and_application_version() {
        check_component_slot_start(2, 4, "RD-6770");
    }
    #[test]
    fn explicit_power_unit_session_uses_generic_handshake_and_application_version() {
        check_component_slot_start(2, 0, "SM-BMR1");
    }
    #[test]
    fn explicit_hub_actuator_session_uses_generic_handshake_and_application_version() {
        check_component_slot_start(6, 4, "MU-S705");
    }
    #[test]
    fn named_lever_probe_resolves_part_before_generic_handshake() {
        let part = (frame(0x48, &[4, 4, 0x0c, 0]), frame(0x48, &[0, 4, 0x0e, 1]));
        let mut commands = discovery_steps(&[4]);
        commands.extend([
            (frame(6, &[4]), frame(0x26, &[0])),
            stock_step(4, 2, 1),
            (frame(6, &[4]), frame(0x26, &[0])),
            part.clone(),
            (frame(6, &[4]), frame(0x26, &[0])),
            stock_step(4, 2, 1),
            part,
        ]);
        commands.extend(component_start_steps(4));
        commands.extend([
            (
                frame(0x48, &[4, 1, 0x3c, 0]),
                frame(0x48, &[0, 1, 0x3e, 1, 2, 3, 4, 5, 6]),
            ),
            stock_step(4, 2, 1),
        ]);
        let (mut session, steps) = scripted(commands);
        let probe = session.probe_component(None, Some("ST-6770-L")).unwrap();
        assert_eq!(probe.identity.cargo, "ST-6770-L");
        assert_eq!(probe.identity.rx_version, "not applicable");
        assert_eq!(probe.serial, "010203040506");
        assert_eq!((probe.model_series, probe.model_unit), (2, 1));
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn component_name_and_slot_must_select_the_same_current_unit() {
        let mut commands = discovery_steps(&[4, 13]);
        commands.extend([(frame(6, &[4]), frame(0x26, &[0])), stock_step(4, 0x23, 2)]);
        let (mut session, steps) = scripted(commands);
        assert!(session
            .normal_start_component(Some(4), Some("RD-6770"))
            .unwrap_err()
            .to_string()
            .contains("matching slots []"));
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn named_motor_is_rechecked_after_inventory_before_pc_link() {
        let model_params = (
            frame(0x48, &[13, 0x16, 0x7c, 0]),
            frame(
                0x48,
                &[0, 0x16, 0x7e, b'0', b'0', b'-', b'C', b'R', b'G', 0],
            ),
        );
        let mut commands = discovery_steps(&[13]);
        commands.extend([
            (frame(6, &[13]), frame(0x26, &[0])),
            stock_step(13, 0x23, 0),
            (frame(6, &[13]), frame(0x26, &[0])),
            model_params.clone(),
            (frame(6, &[13]), frame(0x26, &[0])),
            stock_step(13, 0x30, 0),
            model_params,
        ]);
        let (mut session, steps) = scripted(commands);
        let error = session
            .normal_start_component(Some(13), Some("DU-EP800-CRG"))
            .unwrap_err();
        assert!(error.to_string().contains("no longer matches slot 13"));
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn motor_only_entry_rejects_known_nonmotor_before_pc_link() {
        let mut commands = discovery_steps(&[4]);
        commands.extend([(frame(6, &[4]), frame(0x26, &[0])), stock_step(4, 2, 4)]);
        let (mut session, steps) = scripted(commands);
        assert!(session
            .normal_start(Some(4))
            .unwrap_err()
            .to_string()
            .contains("not a recognized motor"));
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn explicit_unknown_component_is_rejected_before_pc_link() {
        let mut commands = discovery_steps(&[4]);
        commands.extend([(frame(6, &[4]), frame(0x26, &[0])), stock_step(4, 0x99, 2)]);
        let (mut session, steps) = scripted(commands);
        assert!(session
            .normal_start_component(Some(4), None)
            .unwrap_err()
            .to_string()
            .contains("not a catalogued component"));
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn unsupported_drive_unit_is_not_admitted_as_a_component() {
        let mut commands = discovery_steps(&[13]);
        commands.extend([
            (frame(6, &[13]), frame(0x26, &[0])),
            stock_step(13, 0x22, 0),
        ]);
        let (mut session, steps) = scripted(commands);
        let error = session
            .normal_start_component(Some(13), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported drive unit"), "{error}");
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn unresolved_lever_part_is_rejected_before_pc_link() {
        let mut commands = discovery_steps(&[4]);
        commands.extend([
            (frame(6, &[4]), frame(0x26, &[0])),
            stock_step(4, 2, 1),
            (
                frame(0x48, &[4, 4, 0x0c, 0]),
                frame(0x48, &[0, 4, 0x0e, 0xff]),
            ),
        ]);
        let (mut session, steps) = scripted(commands);
        assert!(session
            .normal_start_component(Some(4), None)
            .unwrap_err()
            .to_string()
            .contains("component model remains unresolved"));
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn initialize_waits_for_capacitor_and_ready_state_before_discharge() {
        let (mut session, steps) = scripted(vec![
            (frame(0x10, &[]), frame(0x30, &[0])),
            (frame(0x1a, &[1]), frame(0x3a, &[1, 0])),
            (frame(0x1a, &[2]), frame(0x3a, &[2, 4])),
            // Below the charge threshold: don't query readiness or discharge.
            (frame(0x1c, &[]), frame(0x3c, &2702u16.to_le_bytes())),
            (frame(0x1c, &[]), frame(0x3c, &2703u16.to_le_bytes())),
            // Adequate voltage alone must not enable supply.
            (frame(0x1a, &[2]), frame(0x3a, &[2, 3])),
            (frame(0x1c, &[]), frame(0x3c, &2703u16.to_le_bytes())),
            (frame(0x1a, &[2]), frame(0x3a, &[2, 2])),
            (frame(0x1a, &[3]), frame(0x3a, &[3, 0])),
        ]);
        let power = session.initialize().unwrap();
        assert_eq!(power.raw_state, 2);
        assert!(power.adapter_supply);
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn initialize_preserves_external_supply_after_charging() {
        let (mut session, steps) = scripted(vec![
            (frame(0x10, &[]), frame(0x30, &[0])),
            (frame(0x1a, &[1]), frame(0x3a, &[1, 0])),
            (frame(0x1a, &[2]), frame(0x3a, &[2, 3])),
            (frame(0x1c, &[]), frame(0x3c, &2703u16.to_le_bytes())),
            (frame(0x1a, &[2]), frame(0x3a, &[2, 1])),
        ]);
        let power = session.initialize().unwrap();
        assert_eq!(power.raw_state, 1);
        assert!(!power.adapter_supply);
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn initialize_rejects_truncated_capacitor_reply_without_discharge() {
        let (mut session, steps) = scripted(vec![
            (frame(0x10, &[]), frame(0x30, &[0])),
            (frame(0x1a, &[1]), frame(0x3a, &[1, 0])),
            (frame(0x1a, &[2]), frame(0x3a, &[2, 4])),
            (frame(0x1c, &[]), frame(0x3c, &[0x8f])),
        ]);
        assert!(session
            .initialize()
            .unwrap_err()
            .to_string()
            .contains("short adapter capacitor voltage reply"));
        assert!(!session.adapter_supply);
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn mixed_component_inventory_uses_only_address_selection_and_stock_reads() {
        let (mut session, steps) = scripted(vec![
            (frame(6, &[4]), frame(0x26, &[0])),
            (
                frame(0x48, &[4, 1, 0x1c, 0]),
                frame(0x48, &[0, 1, 0x1e, 0x99, 2]),
            ),
            (frame(6, &[13]), frame(0x26, &[0])),
            (
                frame(0x48, &[13, 1, 0x1c, 0]),
                frame(0x48, &[0, 1, 0x1e, 0x23, 0, 0xff, 0xff]),
            ),
        ]);
        let units = session.inventory_slots(&[4, 13]).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].slot, 4);
        assert!(units[0].model.is_none());
        assert!(units[0].require_motor().is_err());
        units[1].require_motor().unwrap();
        assert_eq!(units[1].raw, [0x23, 0, 0xff, 0xff]);
        assert!(steps.borrow().is_empty());
        assert!(session.normal_slot.is_none());
    }
    #[test]
    fn rejected_inventory_stops_without_retry_or_next_component() {
        let (mut session, steps) = scripted(vec![
            (frame(6, &[4]), frame(0x26, &[0])),
            (frame(0x48, &[4, 1, 0x1c, 0]), frame(0x48, &[0, 1, 0x1f, 1])),
        ]);
        assert!(session.inventory_slots(&[4, 13]).is_err());
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn motor_is_resolved_from_each_current_map_when_pnp_permutes_occupied_slots() {
        let unit = |slot, series, number| {
            UnitIdentity::from_reply(slot, vec![slot, series, number]).unwrap()
        };
        let before = vec![unit(4, 0x23, 1), unit(13, 0x23, 0), unit(19, 0x23, 2)];
        let after = vec![unit(4, 0x23, 0), unit(13, 0x23, 2), unit(19, 0x23, 1)];
        assert_eq!(select_current_motor(&before).unwrap(), 13);
        assert_eq!(select_current_motor(&after).unwrap(), 4);
        assert!(after[1].require_motor().is_err());
        assert!(select_current_motor(&[]).is_err());
        assert!(select_current_motor(&[unit(4, 0x23, 1)]).is_err());
        assert!(select_current_motor(&[unit(4, 0x23, 0), unit(13, 0x35, 0)]).is_err());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn discovered_motor_starts_and_stops_without_a_second_pnp_or_supply_setup() {
        let idle = frame(0x88, &[0x70, 0, 0, 0x78]);
        let mut script = vec![
            (frame(6, &[13]), frame(0x26, &[0])),
            (
                frame(0x48, &[13, 1, 0x1c, 0]),
                frame(0x48, &[0, 1, 0x1e, 0x23, 0]),
            ),
            (frame(4, &[]), frame(0x24, &[0xe0])),
            (frame(0x48, &[13, 0x32, 0x10, 1, 0, 0, 0]), idle.clone()),
        ];
        for (index, key) in [
            [0xa2, 0x2b],
            [0x30, 0x0e],
            [0x7a, 0x4d],
            [0x62, 0x2b],
            [0x85, 0xb4],
        ]
        .into_iter()
        .enumerate()
        {
            script.push((
                frame(0x48, &[13, 0x32, 0x30, key[0], key[1], 0, 0]),
                if index == 4 {
                    frame(0x48, &[0, 0x32, 0x12, 1])
                } else {
                    idle.clone()
                },
            ));
        }
        script.extend([
            (
                frame(0x48, &[13, 0x16, 0x7c, 0]),
                frame(
                    0x48,
                    &[0, 0x16, 0x7e, b'0', b'0', b'-', b'C', b'R', b'G', 0],
                ),
            ),
            (
                frame(0x48, &[13, 1, 0x2c, 0]),
                frame(0x48, &[0, 1, 0x2e, 0x42, 0, 0]),
            ),
            (
                frame(0x48, &[13, 1, 0x84, 1]),
                frame(0x48, &[0, 1, 0x86, 0x41, 11, 0]),
            ),
            (
                frame(0x48, &[13, 0x32, 0x10, 0, 0, 0, 0]),
                frame(0x48, &[0, 0x32, 0x12, 0]),
            ),
        ]);
        // This script accepts no reset, supply, protocol-selection or PnP
        // command: any re-discovery before PC linking fails the test.
        let (mut session, steps) = scripted(script);
        let identity = session.normal_start_discovered(13).unwrap();
        assert_eq!(identity.slot, 13);
        assert_eq!(identity.cargo, "00-CRG");
        assert_eq!(identity.dcas_version, "4.2.0.0");
        assert_eq!(identity.rx_version, "4.1.11.0");
        assert!(session.normal_is_motor);
        session.normal_stop().unwrap();
        assert!(session.normal_slot.is_none());
        assert!(!session.normal_is_motor);
        assert!(steps.borrow().is_empty());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn discovered_slot_still_requires_motor_identity_before_pc_linking() {
        let (mut session, steps) = scripted(vec![
            (frame(6, &[13]), frame(0x26, &[0])),
            (
                frame(0x48, &[13, 1, 0x1c, 0]),
                frame(0x48, &[0, 1, 0x1e, 0x23, 2]),
            ),
        ]);
        assert!(session.normal_start_discovered(13).is_err());
        assert!(session.normal_slot.is_none());
        assert!(!session.normal_is_motor);
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn discovery_and_start_refuse_while_a_pc_link_is_active() {
        // The script admits no command: the guard must refuse before any write.
        let (mut session, steps) = scripted_motor(13, vec![]);
        assert!(session
            .discover()
            .unwrap_err()
            .to_string()
            .contains("still active"));
        assert!(session.units().is_err());
        assert!(session.normal_start(None).is_err());
        assert!(session.probe(Some(13)).is_err());
        assert!(session.probe_component(None, Some("RD-6770")).is_err());
        #[cfg(not(target_arch = "wasm32"))]
        assert!(session.normal_start_discovered(13).is_err());
        assert_eq!(session.normal_slot, Some(13));
        assert!(steps.borrow().is_empty());
    }
    #[test]
    fn names_do_not_grant_motor_access_and_selectors_require_unique_resolved_models() {
        let display = UnitIdentity::from_reply(4, vec![4, 0x23, 2]).unwrap();
        assert_eq!(display.model, Some("SC-EM800"));
        assert!(display.require_motor().is_err());
        assert_eq!(select_named_component(&[display], "scem800").unwrap(), 4);
        let unresolved = UnitIdentity::from_reply(11, vec![11, 0x23, 1]).unwrap();
        assert!(unresolved.model.is_none());
        assert!(select_named_component(&[unresolved], "SW-EM800-L").is_err());
        let duplicates = [
            UnitIdentity::from_reply(4, vec![4, 0x23, 2]).unwrap(),
            UnitIdentity::from_reply(19, vec![19, 0x23, 2]).unwrap(),
        ];
        assert!(select_named_component(&duplicates, "SC-EM800").is_err());
    }
    #[test]
    fn ambiguous_or_missing_motor_selection_is_rejected() {
        assert!(select_motor_slot(&[4, 13], None).is_err());
        assert!(select_motor_slot(&[], None).is_err());
        assert!(select_motor_slot(&[4, 13], Some(12)).is_err());
        assert_eq!(select_motor_slot(&[4, 13], Some(13)).unwrap(), 13);
        assert_eq!(select_motor_slot(&[13], None).unwrap(), 13);
    }
    #[test]
    fn stock_identity_rejects_truncation_or_wrong_slot_and_keeps_unknown_models() {
        assert!(UnitIdentity::from_reply(13, vec![13, 0x23]).is_err());
        assert!(UnitIdentity::from_reply(13, vec![4, 0x23, 0]).is_err());
        assert!(UnitIdentity::from_reply(13, vec![13, 0x23, 1])
            .unwrap()
            .require_motor()
            .is_err());
    }
    #[test]
    fn serial_identity_accepts_only_six_bytes_or_stock_ff_padding() {
        let expected = [1, 2, 3, 4, 5, 6];
        assert_eq!(serial_bytes(&expected).unwrap(), expected);
        assert_eq!(serial_bytes(&[1, 2, 3, 4, 5, 6, 0xff]).unwrap(), expected);
        for invalid in [
            &[1, 2, 3, 4, 5][..],
            &[1, 2, 3, 4, 5, 6, 0][..],
            &[1, 2, 3, 4, 5, 6, 0xff, 0xff][..],
        ] {
            assert!(serial_bytes(invalid).is_err());
        }
    }
    #[test]
    fn component_serial_uses_six_identity_bytes_with_non_ff_transport_tail() {
        let expected = [1, 2, 3, 4, 5, 6];
        let physical = [1, 2, 3, 4, 5, 6, 0x70];
        assert_eq!(component_serial_bytes(&expected).unwrap(), expected);
        assert_eq!(component_serial_bytes(&physical).unwrap(), expected);
        assert!(serial_bytes(&physical).is_err());
        for invalid in [&physical[..5], &[0u8; 8][..]] {
            assert!(component_serial_bytes(invalid).is_err());
        }
    }
    #[test]
    fn idle_fast_notifications_do_not_hide_real_responses() {
        let mut f = Frame {
            control: 0xa8,
            payload: vec![0; 16],
        };
        f.payload[0] = 13;
        f.payload[1] = 0x70;
        assert!(fast_idle_frame(&f));
        f.payload[2] = 0xf2;
        assert!(!fast_idle_frame(&f));
        f.payload[2] = 0;
        f.payload[15] = 1;
        assert!(!fast_idle_frame(&f));
        f.payload[15] = 0;
        f.control = 0x48;
        assert!(!fast_idle_frame(&f));
    }
    #[test]
    fn only_exact_empty_legacy_spi_notifications_are_discarded() {
        assert!(fast_idle_frame(&frame(0x88, &[0x70, 0, 0, 0x78])));
        for f in [
            frame(0x88, &[0x70, 0, 1, 0x79]),
            frame(0x88, &[0x70, 0, 0]),
            frame(0x88, &[0x70, 0, 0, 0x78, 0]),
            frame(0xa8, &[0x70, 0, 0, 0x78]),
            frame(0x88, &[0x31, 0, 0]),
        ] {
            assert!(!fast_idle_frame(&f));
        }
    }
    #[test]
    fn fast_slot_switch_uses_55_and_echoes_assignment() {
        let f = Frame {
            control: 0xa8,
            payload: vec![13, 0x55, 0xff],
        };
        assert!(fast_boot_response_matches(&f, 13, 0x4c, 0xff, 0x55));
        assert!(!fast_boot_response_matches(&f, 13, 0x4c, 13, 0x55));
        assert!(!fast_boot_response_matches(&f, 13, 0x4c, 0xff, 0x31));
        assert!(!fast_boot_response_matches(
            &Frame {
                control: 0x48,
                payload: f.payload
            },
            13,
            0x4c,
            0xff,
            0x55
        ));
    }
    #[test]
    fn normal_response_routes_success_and_error_without_accepting_malformed_frames() {
        let frame = |control, payload| Frame { control, payload };
        assert!(normal_response_matches(
            &frame(0x48, vec![0, 0x16, 0x7e, 0]),
            0,
            0x16,
            0x7e
        ));
        assert!(normal_response_matches(
            &frame(0x48, vec![0, 0x16, 0x7f, 1]),
            0,
            0x16,
            0x7e
        ));
        for bad in [
            frame(0xa8, vec![0, 0x16, 0x7e]),
            frame(0x48, vec![13, 0x16, 0x7e]),
            frame(0x48, vec![0, 0x15, 0x7e]),
            frame(0x48, vec![0, 0x16]),
            frame(0x48, vec![0, 0x16, 0x7d]),
        ] {
            assert!(!normal_response_matches(&bad, 0, 0x16, 0x7e));
        }
    }
    #[test]
    fn unindexed_ack_cannot_accumulate_or_replace_block_2048() {
        let generic = Frame {
            control: 0x88,
            payload: vec![0x31, 0, 0, 0x39],
        };
        let indexed = Frame {
            control: 0x88,
            payload: vec![0x31, 0, 8, 0x41],
        };
        assert!(unindexed_dcas_ack(&generic));
        assert!(!unindexed_dcas_ack(&indexed));
        assert!(!unindexed_dcas_ack(&Frame {
            control: 0x48,
            payload: generic.payload.clone()
        }));
        assert!(!unindexed_dcas_ack(&Frame {
            control: 0x88,
            payload: vec![0x31]
        }));
        let mut queue = VecDeque::new();
        for _ in 0..2469 {
            queue.push_back(generic.clone());
            queue.retain(|f| !unindexed_dcas_ack(f));
            assert!(queue.is_empty());
        }
    }
    #[test]
    fn legacy_dcas_buffer_fixture() {
        let mut block = [0xff; 64];
        block[..3].copy_from_slice(&[0x31, 0x41, 0x59]);
        let sum = block.iter().fold(0u8, |a, &b| a.wrapping_add(b));
        let p = dcas_buffer_packet(0x83, 2469, &block, sum);
        assert_eq!(p.len(), 70);
        assert_eq!(&p[..6], &[0x83, 2, 0, 0x31, 0x41, 0x59]);
        assert_eq!(&p[67..], &[0x8e, 0xa5, 0x09]);
    }
}

#[cfg(test)]
#[path = "device_unit_auth_tests.rs"]
mod unit_auth_tests;

#[cfg(test)]
#[path = "device_boot_tests.rs"]
mod boot_tests;

impl crate::firmware_io::FirmwareIo for Session {
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        Session::send(self, control, payload)
    }
    fn wait(&mut self, timeout: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
        Session::wait(self, timeout, check)
    }
    fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool) {
        self.incoming.retain(|f| !check(f));
    }
}
