//! Native USB recovery for the EP800 key-2 stall, using a temporary board bridge.
//! The handshake and ordinary D writer retain one exclusive, uninterrupted port.
use crate::{
    device::{BootIdentity, Session},
    firmware::{Firmware, FirmwareKind, Version},
    platform::{self, Port},
    protocol::{self, Frame},
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const D_SOURCE: &str = "35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543";
const D_DECODED: &str = "f2f2d18441b7651620a0aacfd4f9f31b2a97b6397bf0f03b2171ff9652cf2551";
const M_HASH: &str = "94986ebf8c21587a2b54b197f19a51c4118918bdca0aff6296a7727f7dfacc44";
const KEYS: [[u8; 3]; 5] = [
    [0x31, 0x41, 0x59],
    [0x26, 0x53, 0x58],
    [0x97, 0x93, 0x23],
    [0x84, 0x62, 0x64],
    [0x33, 0x83, 0x27],
];
const WINDOW: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_millis(100);
const MAX_TX: u16 = 120;

pub struct BridgeOptions<'a> {
    pub port: &'a str,
    pub dcas: &'a Path,
    pub companion: &'a Path,
    pub serial: Option<&'a str>,
    pub journal: &'a Path,
    pub trace: Option<&'a Path>,
    pub countdown: u64,
    pub attempts: u8,
    pub write: bool,
}

/// A preflight can leave the serial unknown; an executed restore reads it from
/// the connected motor's bootloader and binds the writer to that identity.
pub struct BridgeReport {
    pub serial: Option<String>,
    pub transferred: bool,
}

struct Journal(File);
impl Journal {
    fn new(path: &Path) -> Result<Self> {
        Ok(Self(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .context("journal must be a new file")?,
        ))
    }
    fn record(&mut self, stage: &str, data: Value) -> Result<()> {
        writeln!(
            self.0,
            "{}",
            json!({"time_ms":platform::now_ms(),"stage":stage,"data":data})
        )?;
        self.0.sync_all()?;
        Ok(())
    }
}

struct Received {
    frame: Frame,
    ingress: Instant,
}

/// A response belongs to the request that existed when its first byte arrived,
/// rather than when a split frame was finally decoded. Framing errors halt.
#[derive(Default)]
struct TimedDecoder {
    start: Option<Instant>,
    escaped: bool,
    data: Vec<u8>,
}
impl TimedDecoder {
    fn has_partial(&self) -> bool {
        self.start.is_some()
    }
    fn feed(&mut self, bytes: &[u8], ingress: Instant) -> Result<Vec<Received>> {
        let mut out = Vec::new();
        for &b in bytes {
            let Some(start) = self.start else {
                ensure!(b == 0xff || b == 0xbb, "unframed RX byte {b:02x}");
                if b == 0xbb {
                    self.start = Some(ingress);
                }
                continue;
            };
            if b == 0xbb {
                ensure!(!self.escaped, "truncated RX escape");
                if self.data.is_empty() {
                    self.start = Some(ingress);
                    continue;
                }
                ensure!(self.data.len() >= 2, "short RX frame");
                ensure!(
                    self.data.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0,
                    "RX checksum"
                );
                out.push(Received {
                    frame: Frame {
                        control: self.data[0],
                        payload: self.data[1..self.data.len() - 1].to_vec(),
                    },
                    ingress: start,
                });
                self.start = None;
                self.data.clear();
            } else if self.escaped {
                ensure!(matches!(b, 0x9b | 0x9d), "invalid RX escape");
                self.data.push(b ^ 0x20);
                self.escaped = false;
            } else if b == 0xbd {
                self.escaped = true;
            } else {
                self.data.push(b);
            }
            ensure!(self.data.len() <= 4096, "RX frame overflow");
        }
        Ok(out)
    }
}

struct Sequence {
    start: Instant,
    expected: u8,
    epoch: u32,
    acks: Vec<u8>,
    progress: bool,
    full: bool,
    outstanding: bool,
    next_send: Instant,
    tx_count: u16,
    last_tx: Option<Instant>,
}
impl Sequence {
    fn new(start: Instant) -> Self {
        Self {
            start,
            expected: 1,
            epoch: 0,
            acks: Vec::new(),
            progress: false,
            full: false,
            outstanding: false,
            next_send: start + Duration::from_millis(60),
            tx_count: 0,
            last_tx: None,
        }
    }
    fn can_send(&self, now: Instant) -> bool {
        now < self.start + WINDOW
            && now >= self.next_send
            && self.tx_count < MAX_TX
            && !self.full
            && !(self.progress && self.outstanding)
    }
    fn sent(&mut self, now: Instant) -> Result<()> {
        ensure!(self.can_send(now), "key TX timing/state/cap guard rejected");
        self.tx_count += 1;
        self.last_tx = Some(now);
        self.outstanding = true;
        self.next_send = now + RETRY;
        Ok(())
    }
    fn ack(&mut self, key: u8, now: Instant, ingress: Instant) -> Result<()> {
        ensure!(
            self.last_tx.is_some_and(|tx| ingress >= tx),
            "ACK predates outstanding TX"
        );
        ensure!(
            self.outstanding && key == self.expected,
            "unexpected/duplicate ACK{key}, expected {}",
            self.expected
        );
        self.acks.push(key);
        self.progress |= key >= 2;
        self.outstanding = false;
        if key == 5 {
            self.full = true;
        } else {
            self.expected += 1;
            self.next_send = now;
        }
        Ok(())
    }
    fn nak(&mut self, now: Instant, ingress: Instant) -> Result<()> {
        ensure!(
            self.last_tx.is_some_and(|tx| ingress >= tx),
            "NAK predates outstanding TX"
        );
        ensure!(!self.progress, "NAK after ACK2; preserving state");
        ensure!(self.outstanding, "NAK without outstanding key");
        self.epoch += 1;
        self.expected = 1;
        self.acks.clear();
        self.outstanding = false;
        self.next_send = now + RETRY;
        Ok(())
    }
}

struct Wire {
    port: Box<dyn Port>,
    decoder: TimedDecoder,
    queue: VecDeque<Received>,
    trace: File,
    epoch: Instant,
    seq: Sequence,
    last_send: Option<Instant>,
}
impl Wire {
    fn new(port: Box<dyn Port>, trace: File) -> Self {
        let now = Instant::now();
        Self {
            port,
            trace,
            decoder: TimedDecoder::default(),
            queue: VecDeque::new(),
            epoch: now,
            seq: Sequence::new(now),
            last_send: None,
        }
    }
    fn log(&mut self, direction: &str, bytes: &[u8]) -> Result<()> {
        writeln!(
            self.trace,
            "{}",
            json!({"time_ms":platform::now_ms(),"elapsed_us":self.epoch.elapsed().as_micros(),"direction":direction,"bytes":hex::encode(bytes)})
        )?;
        self.trace.flush()?;
        Ok(())
    }
    fn event(&mut self, event: &str) -> Result<()> {
        writeln!(
            self.trace,
            "{}",
            json!({"time_ms":platform::now_ms(),"event":event,"epoch":self.seq.epoch,"expected_key":self.seq.expected,"ordered_key_acks":self.seq.acks,"progress_seen":self.seq.progress,"key_tx_count":self.seq.tx_count})
        )?;
        self.trace.flush()?;
        Ok(())
    }
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        ensure!(
            control == 0x88 || self.seq.tx_count == 0,
            "adapter reconfiguration after first motor key is forbidden"
        );
        let allowed = match control {
            0x10 | 4 | 0x1c => payload.is_empty(),
            0x1a => matches!(payload, [1] | [2] | [3] | [6]),
            3 => payload == [0x60],
            6 => payload == [31],
            7 => payload == [0, 1, 0, 0, 0] || payload == [5],
            0x88 => {
                payload.len() == 4
                    && payload[0] == self.seq.expected
                    && payload[1..] == KEYS[(self.seq.expected - 1) as usize]
                    && self.seq.can_send(Instant::now())
            }
            _ => false,
        };
        ensure!(allowed, "bridge recovery command allowlist rejected TX");
        if control == 0x88 {
            self.seq.sent(Instant::now())?;
        }
        let bytes = protocol::encode(control, payload);
        self.log("tx", &bytes)?;
        self.port.set_timeout(Duration::from_millis(20))?;
        self.last_send = Some(Instant::now());
        self.port
            .write_all(&bytes)
            .context("write outcome uncertain; no retry or reset")?;
        #[cfg(not(windows))]
        self.port.flush()?;
        self.log("tx-host-complete", &[])?;
        if control == 0x88 {
            self.seq.next_send = Instant::now() + RETRY;
        }
        Ok(())
    }
    fn next(&mut self, deadline: Instant) -> Result<Option<Received>> {
        loop {
            if let Some(received) = self.queue.pop_front() {
                let mut bytes = vec![received.frame.control];
                bytes.extend_from_slice(&received.frame.payload);
                self.log("rx-frame", &bytes)?;
                return Ok(Some(received));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            #[cfg(windows)]
            if remaining < Duration::from_millis(1) {
                std::thread::sleep(remaining);
                return Ok(None);
            }
            self.port
                .set_timeout(remaining.min(Duration::from_millis(2)))?;
            let mut bytes = [0; 4096];
            match self.port.read(&mut bytes) {
                Ok(0) => {}
                Ok(n) => {
                    let ingress = Instant::now();
                    self.log("rx", &bytes[..n])?;
                    self.queue.extend(self.decoder.feed(&bytes[..n], ingress)?);
                    ensure!(self.queue.len() <= 2048, "RX queue overflow");
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e).context("read failed; no retry or reset"),
            }
        }
    }
    fn reply(&mut self) -> Result<Frame> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Some(received) = self.next(deadline)? {
            if !empty_spi(&received.frame) {
                ensure!(
                    self.last_send.is_some_and(|tx| received.ingress >= tx),
                    "adapter response predates outstanding request"
                );
                return Ok(received.frame);
            }
        }
        bail!("adapter acknowledgement deadline expired")
    }
    fn ack(&mut self, c: u8, p: &[u8], rc: u8, rp: &[u8]) -> Result<()> {
        self.send(c, p)?;
        let f = self.reply()?;
        ensure!(
            f.control == rc && f.payload == rp,
            "unexpected adapter reply {:02x}:{}",
            f.control,
            hex::encode(f.payload)
        );
        Ok(())
    }
    fn stop_before_keys(&mut self) -> Result<()> {
        ensure!(self.seq.tx_count == 0, "cannot stop supply after keys");
        let bytes = protocol::encode(0x1a, &[6]);
        // A trace failure must not prevent stopping supply after setup failure.
        let _ = self.log("cleanup-tx", &bytes);
        self.port.set_timeout(Duration::from_millis(20))?;
        self.port.write_all(&bytes)?;
        #[cfg(not(windows))]
        self.port.flush()?;
        let mut decoder = protocol::Decoder::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            ensure!(Instant::now() < deadline, "STOP6 acknowledgement missing");
            self.port.set_timeout(Duration::from_millis(2))?;
            let mut bytes = [0u8; 4096];
            match self.port.read(&mut bytes) {
                Ok(n) if n > 0 => {
                    let _ = self.log("cleanup-rx", &bytes[..n]);
                    // Cleanup is terminal: tolerate remnants of the failed
                    // setup, but require a correctly framed/checksummed ACK.
                    for frame in decoder.feed(&bytes[..n]).into_iter().flatten() {
                        if frame.control == 0x3a && frame.payload == [6, 0] {
                            let _ = self.log("cleanup-stop6-ack", &[0x3a, 6, 0]);
                            return Ok(());
                        }
                    }
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e).context("STOP6 read uncertain"),
            }
        }
    }
    fn prepare(&mut self, mut pause: impl FnMut(Duration)) -> Result<()> {
        self.ack(0x10, &[], 0x30, &[0])?;
        pause(Duration::from_secs(3));
        self.ack(0x1a, &[1], 0x3a, &[1, 0])?;
        let limit = Instant::now() + Duration::from_secs(90);
        loop {
            ensure!(Instant::now() < limit, "adapter readiness deadline");
            self.send(0x1c, &[])?;
            let cap = self.reply()?;
            ensure!(
                cap.control == 0x3c && cap.payload.len() == 2,
                "wrong capacitor reply"
            );
            let raw = u16::from_le_bytes(cap.payload[..2].try_into().unwrap());
            if raw != 0xffff && raw >= 2704 {
                self.send(0x1a, &[2])?;
                let state = self.reply()?;
                ensure!(
                    state.control == 0x3a && state.payload.len() == 2 && state.payload[0] == 2,
                    "wrong adapter state reply"
                );
                if state.payload[1] == 2 {
                    break;
                }
                ensure!(
                    matches!(state.payload[1], 3 | 4),
                    "adapter is not ready/charging; disconnect battery"
                );
            }
            pause(Duration::from_secs(3));
        }
        self.ack(3, &[0x60], 0x23, &[0])?;
        pause(Duration::from_secs(1));
        self.send(4, &[])?;
        let mode = self.reply()?;
        ensure!(
            mode.control == 0x24 && mode.payload == [0xe0],
            "wrong adapter mode"
        );
        self.ack(6, &[31], 0x26, &[0])?;
        self.ack(7, &[0, 1, 0, 0, 0], 0x27, &[0, 0])?;
        self.ack(7, &[5], 0x27, &[5, 0])?;
        self.ack(0x1a, &[3], 0x3a, &[3, 0])?;
        self.seq = Sequence::new(Instant::now());
        self.event("epoch-start")?;
        Ok(())
    }
    fn complete(&self) -> bool {
        self.seq.full && self.queue.is_empty() && !self.decoder.has_partial()
    }
    /// False only for an ordinary clean expiry before ACK2. Every ambiguous
    /// transport, framing, sequence, or post-ACK2 failure is an error, not a retry.
    fn handshake(&mut self, notify: &mut impl FnMut(&str)) -> Result<bool> {
        loop {
            if self.complete() {
                self.trace.sync_all()?;
                return Ok(true);
            }
            let now = Instant::now();
            if now >= self.seq.start + WINDOW || self.seq.tx_count >= MAX_TX {
                ensure!(
                    !self.seq.progress,
                    "key window ended after ACK2; preserving state, no retry"
                );
                ensure!(
                    !self.decoder.has_partial(),
                    "key window ended with partial response; no retry"
                );
                ensure!(
                    self.queue.iter().all(|r| empty_spi(&r.frame)),
                    "unconsumed response at key-window end; no retry"
                );
                self.queue.clear();
                return Ok(false);
            }
            if self.queue.is_empty() && !self.decoder.has_partial() && self.seq.can_send(now) {
                let key = self.seq.expected;
                let k = KEYS[(key - 1) as usize];
                self.send(0x88, &[key, k[0], k[1], k[2]])?;
                self.event("key-tx")?;
            }
            let mut deadline = self.seq.start + WINDOW;
            if !self.decoder.has_partial() && !(self.seq.progress && self.seq.outstanding) {
                deadline = deadline.min(self.seq.next_send);
            }
            if let Some(r) = self.next(deadline)? {
                if empty_spi(&r.frame) {
                    continue;
                }
                ensure!(
                    r.ingress < self.seq.start + WINDOW,
                    "response arrived after key window"
                );
                let f = r.frame;
                ensure!(
                    f.control == 0x88 && f.payload.len() == 4,
                    "unexpected key response {:02x}:{}",
                    f.control,
                    hex::encode(&f.payload)
                );
                if f.payload[0] == 0x11 {
                    let key = f.payload[1];
                    ensure!(
                        (1..=5).contains(&key) && f.payload == [0x11, key, 0, 0x19 + key],
                        "malformed key ACK"
                    );
                    self.seq.ack(key, Instant::now(), r.ingress)?;
                    self.event("ack")?;
                    notify(&format!(
                        "Key {key}/5 acknowledged. Keep the bridge connected."
                    ));
                } else if f.payload == [0x12, 0, 0, 0x1a] {
                    self.seq.nak(Instant::now(), r.ingress)?;
                    self.event("nak-restart")?;
                } else {
                    bail!(
                        "unknown/malformed key response {}; no retry",
                        hex::encode(f.payload)
                    );
                }
            }
        }
    }
    fn into_session(self) -> Result<Session> {
        ensure!(
            self.complete(),
            "incomplete handshake cannot authorize firmware restore"
        );
        Ok(Session::from_bridge_port(self.port, self.trace))
    }
}

fn empty_spi(f: &Frame) -> bool {
    f.control == 0x88 && f.payload == [0x70, 0, 0, 0x78]
}

fn validate_stock(d: &Firmware, m: &Firmware) -> Result<()> {
    ensure!(
        d.metadata.source_sha256 == D_SOURCE && d.metadata.decoded_sha256 == D_DECODED,
        "bridge recovery requires the supported original D firmware file"
    );
    ensure!(
        m.metadata.source_sha256 == M_HASH && m.metadata.decoded_sha256 == M_HASH,
        "bridge recovery requires the supported original M companion file (validation only)"
    );
    ensure!(
        d.metadata.kind == FirmwareKind::DcasX
            && d.metadata.version == Version([4, 2, 0, 0])
            && d.metadata.compatibility_version == Version([4, 1, 11, 0])
            && d.metadata.board == Some(1)
            && d.metadata.series == 0x23
            && d.metadata.unit == 0
            && d.data.len() == 157996
            && d.metadata.checksum == 0xcb
            && m.metadata.kind == FirmwareKind::Renesas
            && m.metadata.version == Version([4, 1, 11, 0]),
        "unsupported stock firmware metadata"
    );
    d.validate_companion(m)
}

fn validate_live(boot: &BootIdentity, expected_serial: Option<&[u8; 6]>) -> Result<()> {
    ensure!(
        expected_serial.is_none_or(|serial| boot.serial == *serial)
            && boot.serial != [0; 6]
            && boot.serial != [0xff; 6]
            && boot.version == [5, 0, 6, 1]
            && boot.board == 1
            && boot.series == 0x23
            && boot.unit == 0
            && boot.slot == 31,
        "motor serial, bootloader, or board identity mismatch; no firmware written"
    );
    Ok(())
}

fn output_path(path: &Path) -> Result<PathBuf> {
    ensure!(!path.exists(), "output already exists: {}", path.display());
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).context("output directory must already exist")?;
    ensure!(parent.is_dir(), "output parent is not a directory");
    Ok(parent.join(path.file_name().context("output file name required")?))
}

/// Validate offline unless `write` is requested. A write opens only native USB,
/// completes all five keys, checks identity, then calls the regular D writer.
/// No M firmware or settings write is performed.
pub fn recover_bridge(
    options: BridgeOptions<'_>,
    mut notify: impl FnMut(&str),
) -> Result<BridgeReport> {
    ensure!(
        options.port == "usb",
        "bridge recovery requires --port usb (native USB)"
    );
    ensure!(
        (1..=20).contains(&options.attempts),
        "bridge attempts must be 1..=20"
    );
    ensure!(
        options.countdown <= 300,
        "bridge countdown must be at most 300 seconds"
    );
    let serial: Option<[u8; 6]> = options
        .serial
        .map(|serial| {
            hex::decode(serial)
                .context("serial must contain 12 hexadecimal digits")?
                .try_into()
                .map_err(|_| anyhow::anyhow!("serial must contain exactly 12 hexadecimal digits"))
        })
        .transpose()?;
    let d = Firmware::load(options.dcas)?;
    let m = Firmware::load(options.companion)?;
    validate_stock(&d, &m)?;
    let default_trace = options.journal.with_extension("wire.jsonl");
    let trace_path = output_path(options.trace.unwrap_or(&default_trace))?;
    let journal_path = output_path(options.journal)?;
    ensure!(
        trace_path != journal_path,
        "journal and trace paths must differ"
    );
    if !options.write {
        notify("Bridge recovery preflight passed. D 4.2.0.0 will be restored; M is validated only. No hardware opened. Add --write to execute.");
        if serial.is_none() {
            notify("Motor serial will be read from the bootloader after the recovery connection is established.");
        }
        return Ok(BridgeReport {
            serial: serial.map(hex::encode),
            transferred: false,
        });
    }
    let mut journal = Journal::new(&journal_path)?;
    let trace = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&trace_path)
        .context("trace must be a new file")?;
    trace.sync_all()?;
    journal.record("inputs", json!({"d":d.metadata,"m":m.metadata,"expected_serial":serial.map(hex::encode),"trace":trace_path,"attempts":options.attempts,"countdown":options.countdown,"scope":"stock D only; M compatibility check; no settings write"}))?;
    notify(&format!(
        "Journal: {}\nWire trace: {}",
        journal_path.display(),
        trace_path.display()
    ));
    notify("Hold the temporary 20 ohm bridge securely. Keep it connected until finish and reset are acknowledged.");
    for remaining in (1..=options.countdown).rev() {
        notify(&format!("Starting in {remaining}..."));
        std::thread::sleep(Duration::from_secs(1));
    }
    journal.record("open-usb", json!({}))?;
    let port = platform::open_port("usb")?;
    let mut wire = Wire::new(port, trace);
    for attempt in 1..=options.attempts {
        journal.record("key-attempt", json!({"attempt":attempt}))?;
        notify(&format!(
            "Recovery connection attempt {attempt}/{}.",
            options.attempts
        ));
        // Only reached for the initial attempt or a clean, pre-ACK2 expiry.
        wire.seq = Sequence::new(Instant::now());
        if let Err(error) = wire.prepare(std::thread::sleep) {
            let stop = wire.stop_before_keys();
            journal.record(
                "setup-failed",
                json!({"error":format!("{error:#}"),"stop_acknowledged":stop.is_ok()}),
            )?;
            return Err(error.context(format!(
                "adapter setup failed; stop result: {stop:?}; no automatic retry"
            )));
        }
        let complete = match wire.handshake(&mut notify) {
            Ok(complete) => complete,
            Err(error) => {
                journal.record(
                    "key-sequence-halted",
                    json!({"error":format!("{error:#}"),"acks":wire.seq.acks}),
                )?;
                return Err(error.context("connection state uncertain; no automatic retry/reset"));
            }
        };
        journal.record("key-attempt-result", json!({"attempt":attempt,"complete":complete,"acks":wire.seq.acks,"epochs":wire.seq.epoch,"tx":wire.seq.tx_count}))?;
        if complete {
            notify("All five keys acknowledged. Checking motor identity; keep holding the bridge.");
            let session = wire.into_session()?;
            return restore(session, &d, serial.as_ref(), &mut journal, &mut notify);
        }
        notify(
            "Key 2 did not complete. Adjust the bridge while the next connection attempt starts.",
        );
    }
    bail!("no complete recovery handshake; no firmware written; inspect journal and trace")
}

fn restore(
    mut session: Session,
    d: &Firmware,
    serial: Option<&[u8; 6]>,
    journal: &mut Journal,
    notify: &mut impl FnMut(&str),
) -> Result<BridgeReport> {
    let boot = session.boot_identity()?;
    journal.record("fresh-boot-identity", json!(boot))?;
    validate_live(&boot, serial)?;
    d.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))?;
    let detected_serial = hex::encode(boot.serial);
    journal.record("verified-motor-serial", json!({"serial":detected_serial,"source":"connected motor bootloader","expected_serial":serial.map(hex::encode)}))?;
    notify(&format!(
        "Motor serial: {detected_serial}. Restoring stock D; keep the bridge connected."
    ));
    journal.record("destructive-start", json!({"d":d.metadata,"boot":boot,"note":"regular D writer rechecks identity before erase; no M/settings write"}))?;
    let receipt = match session.flash_dcas(&d.data, &boot, |done, total| {
        if done == total || done % 8192 == 0 {
            notify(&format!(
                "Stock D {done}/{total} bytes. Keep the bridge connected."
            ));
        }
    }) {
        Ok(receipt) => receipt,
        Err(error) => {
            journal.record("uncertain", json!(format!("{error:#}")))?;
            return Err(
                error.context("stock restore outcome uncertain; do not automatically retry/reset")
            );
        }
    };
    journal.record("finish-and-reset", json!(receipt))?;
    ensure!(
        receipt.bytes == 157996
            && receipt.checksum == 0xcb
            && receipt.finish_acknowledged
            && receipt.reset_acknowledged,
        "incomplete stock restore receipt; inspect journal"
    );
    journal.record("post-verification-pending", json!({"required":"remove bridge; verify normal communication, original serial, D4.2.0.0/M4.1.11.0 and saved settings"}))?;
    notify("Stock transfer finish/reset acknowledged. Remove the resistor, then verify normal communication and settings.");
    Ok(BridgeReport {
        serial: Some(detected_serial),
        transferred: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{firmware::FirmwareMetadata, protocol::Decoder};
    use std::{
        cell::RefCell,
        io::Read,
        rc::Rc,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
    const SERIAL: [u8; 6] = [1, 2, 3, 4, 5, 6];
    fn temp_file() -> (PathBuf, File) {
        let path = std::env::temp_dir().join(format!(
            "cranklink-bridge-test-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        (path, file)
    }
    struct Step {
        tx: Frame,
        rx: Vec<u8>,
    }
    struct MockPort {
        steps: Rc<RefCell<VecDeque<Step>>>,
        pending: VecDeque<u8>,
        fail_write: bool,
    }
    impl Read for MockPort {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if self.pending.is_empty() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let n = bytes.len().min(self.pending.len());
            for b in &mut bytes[..n] {
                *b = self.pending.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    impl Write for MockPort {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail_write {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let actual = Decoder::default().feed(bytes).remove(0).unwrap();
            let step = self
                .steps
                .borrow_mut()
                .pop_front()
                .expect("unexpected write; reset/reinitialize/erase must not occur");
            assert_eq!(actual, step.tx);
            self.pending.extend(step.rx);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Port for MockPort {
        fn set_timeout(&mut self, _: Duration) -> io::Result<()> {
            Ok(())
        }
    }
    fn step(c: u8, p: &[u8], rc: u8, rp: &[u8]) -> Step {
        Step {
            tx: Frame {
                control: c,
                payload: p.to_vec(),
            },
            rx: protocol::encode(rc, rp),
        }
    }
    fn mock(steps: Vec<Step>) -> (Wire, Rc<RefCell<VecDeque<Step>>>, PathBuf) {
        let steps = Rc::new(RefCell::new(steps.into()));
        let port = MockPort {
            steps: steps.clone(),
            pending: VecDeque::new(),
            fail_write: false,
        };
        let (path, trace) = temp_file();
        (Wire::new(Box::new(port), trace), steps, path)
    }
    fn setup_steps() -> Vec<Step> {
        vec![
            step(0x10, &[], 0x30, &[0]),
            step(0x1a, &[1], 0x3a, &[1, 0]),
            step(0x1c, &[], 0x3c, &2703u16.to_le_bytes()),
            step(0x1c, &[], 0x3c, &2704u16.to_le_bytes()),
            step(0x1a, &[2], 0x3a, &[2, 3]),
            step(0x1c, &[], 0x3c, &2705u16.to_le_bytes()),
            step(0x1a, &[2], 0x3a, &[2, 2]),
            step(3, &[0x60], 0x23, &[0]),
            step(4, &[], 0x24, &[0xe0]),
            step(6, &[31], 0x26, &[0]),
            step(7, &[0, 1, 0, 0, 0], 0x27, &[0, 0]),
            step(7, &[5], 0x27, &[5, 0]),
            step(0x1a, &[3], 0x3a, &[3, 0]),
        ]
    }
    fn keys() -> Vec<Step> {
        (1..=5)
            .map(|key| {
                let k = KEYS[(key - 1) as usize];
                step(
                    0x88,
                    &[key, k[0], k[1], k[2]],
                    0x88,
                    &[0x11, key, 0, 0x19 + key],
                )
            })
            .collect()
    }
    fn identity_steps(serial: &[u8; 6]) -> Vec<Step> {
        vec![
            step(0x88, &[0x2e, 0, 0, 0], 0x88, &[0x3a, 0x50, 6]),
            step(0x88, &[0x2f, 0, 0, 0], 0x88, &[0x3b, 1, 1]),
            step(0x88, &[0x30, 0, 0, 0], 0x88, &[0x3c, 0x23, 0]),
            step(
                0x88,
                &[0x29, 0, 0, 0],
                0x88,
                &[0x35, serial[0], serial[1], serial[2]],
            ),
            step(
                0x88,
                &[0x2a, 0, 0, 0],
                0x88,
                &[0x36, serial[3], serial[4], serial[5]],
            ),
        ]
    }
    fn stock() -> (Firmware, Firmware) {
        let metadata = FirmwareMetadata {
            kind: FirmwareKind::DcasX,
            version: Version([4, 2, 0, 0]),
            compatibility_version: Version([4, 1, 11, 0]),
            series: 0x23,
            unit: 0,
            board: Some(1),
            encrypted: true,
            source_bytes: 158052,
            decoded_bytes: 157996,
            source_sha256: D_SOURCE.into(),
            decoded_sha256: D_DECODED.into(),
            checksum: 0xcb,
        };
        let mut d = Firmware {
            metadata,
            data: vec![0; 157996],
        };
        d.data[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
        let mut m = d.clone();
        m.metadata.kind = FirmwareKind::Renesas;
        m.metadata.version = Version([4, 1, 11, 0]);
        m.metadata.source_sha256 = M_HASH.into();
        m.metadata.decoded_sha256 = M_HASH.into();
        (d, m)
    }
    #[test]
    fn readiness_routing_keys_and_handover_use_one_port_without_reinitializing() {
        let mut script = setup_steps();
        script.extend(keys());
        let mut identity = identity_steps(&SERIAL);
        // The exact empty SPI frame must not accumulate in Session's queue.
        let mut flood = protocol::encode(0x88, &[0x70, 0, 0, 0x78]);
        flood.extend(identity[0].rx.clone());
        identity[0].rx = flood;
        script.extend(identity);
        let (mut wire, steps, path) = mock(script);
        let mut pauses = Vec::new();
        wire.prepare(|d| pauses.push(d)).unwrap();
        assert_eq!(
            pauses,
            [
                Duration::from_secs(3),
                Duration::from_secs(3),
                Duration::from_secs(3),
                Duration::from_secs(1)
            ]
        );
        assert!(wire.handshake(&mut |_| {}).unwrap());
        let mut session = wire.into_session().unwrap();
        let boot = session.boot_identity().unwrap();
        validate_live(&boot, Some(&SERIAL)).unwrap();
        assert!(steps.borrow().is_empty());
        drop(session);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn wrong_live_serial_prevents_writer_and_erase() {
        let mut script = keys();
        script.extend(identity_steps(&[9, 2, 3, 4, 5, 6]));
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.handshake(&mut |_| {}).unwrap());
        let session = wire.into_session().unwrap();
        let (path, file) = temp_file();
        let mut journal = Journal(file);
        let (d, _) = stock();
        assert!(restore(session, &d, Some(&SERIAL), &mut journal, &mut |_| {}).is_err());
        assert!(steps.borrow().is_empty());
        drop(journal);
        fs::remove_file(path).unwrap();
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn writer_rechecks_identity_and_does_not_erase_changed_motor() {
        let mut script = keys();
        script.extend(identity_steps(&SERIAL));
        script.extend(identity_steps(&[9, 2, 3, 4, 5, 6]));
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.handshake(&mut |_| {}).unwrap());
        let session = wire.into_session().unwrap();
        let (path, file) = temp_file();
        let mut journal = Journal(file);
        let (d, _) = stock();
        assert!(restore(session, &d, Some(&SERIAL), &mut journal, &mut |_| {}).is_err());
        assert!(steps.borrow().is_empty());
        drop(journal);
        fs::remove_file(path).unwrap();
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn unknown_serial_is_read_on_same_port_and_bound_to_writer_identity() {
        let mut script = keys();
        script.extend(identity_steps(&SERIAL));
        // Detection must not turn the writer's fresh identity check into an
        // unchecked write. A changed serial must stop before erase.
        script.extend(identity_steps(&[9, 2, 3, 4, 5, 6]));
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.handshake(&mut |_| {}).unwrap());
        let session = wire.into_session().unwrap();
        let (path, file) = temp_file();
        let mut journal = Journal(file);
        let (d, _) = stock();
        let mut notices = Vec::new();
        assert!(restore(session, &d, None, &mut journal, &mut |s| {
            notices.push(s.to_owned());
        })
        .is_err());
        assert!(steps.borrow().is_empty());
        assert!(notices.iter().any(|s| s.contains("010203040506")));
        drop(journal);
        let events: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let detected = events
            .iter()
            .find(|event| event["stage"] == "verified-motor-serial")
            .unwrap();
        assert_eq!(detected["data"]["serial"], "010203040506");
        assert!(detected["data"]["expected_serial"].is_null());
        fs::remove_file(path).unwrap();
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn buffered_ack2_before_tx2_is_rejected() {
        let mut script = keys();
        script[0]
            .rx
            .extend(protocol::encode(0x88, &[0x11, 2, 0, 0x1b]));
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.handshake(&mut |_| {}).is_err());
        assert_eq!(wire.seq.tx_count, 1);
        assert_eq!(steps.borrow().len(), 4);
        drop(wire);
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn exact_nak_before_ack2_restarts_only_the_local_key_epoch() {
        let mut script = keys();
        script.truncate(2);
        script[1].rx = protocol::encode(0x88, &[0x12, 0, 0, 0x1a]);
        script.extend(keys());
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.handshake(&mut |_| {}).unwrap());
        assert_eq!(wire.seq.epoch, 1);
        assert_eq!(wire.seq.acks, [1, 2, 3, 4, 5]);
        assert_eq!(wire.seq.tx_count, 7);
        assert!(steps.borrow().is_empty());
        drop(wire);
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn stale_ack1_buffered_with_nak_cannot_advance_new_epoch() {
        let mut script = keys();
        script[0].rx = protocol::encode(0x88, &[0x12, 0, 0, 0x1a]);
        script[0]
            .rx
            .extend(protocol::encode(0x88, &[0x11, 1, 0, 0x1a]));
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.handshake(&mut |_| {}).is_err());
        assert_eq!(wire.seq.tx_count, 1);
        assert_eq!(wire.seq.epoch, 1);
        assert!(wire.seq.acks.is_empty());
        assert_eq!(steps.borrow().len(), 4);
        drop(wire);
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn wrong_mode_aborts_before_adapter_power_on_or_motor_keys() {
        let mut script = setup_steps();
        script.truncate(9);
        script[8].rx = protocol::encode(0x24, &[0xc0]);
        let (mut wire, steps, trace) = mock(script);
        assert!(wire.prepare(|_| {}).is_err());
        assert_eq!(wire.seq.tx_count, 0);
        assert!(steps.borrow().is_empty());
        drop(wire);
        fs::remove_file(trace).unwrap();
    }
    #[test]
    fn setup_cleanup_reads_stop_ack_even_when_trace_writes_fail() {
        let (mut wire, steps, path) = mock(vec![step(0x1a, &[6], 0x3a, &[6, 0])]);
        // A read-only trace deterministically fails each attempted log write.
        wire.trace = File::open(&path).unwrap();
        assert!(wire.log("test", &[]).is_err());
        wire.stop_before_keys().unwrap();
        assert!(steps.borrow().is_empty());
        assert_eq!(wire.seq.tx_count, 0);
        drop(wire);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn ack5_with_nak_unexpected_or_partial_tail_cannot_handover() {
        for tail in [
            protocol::encode(0x88, &[0x12, 0, 0, 0x1a]),
            protocol::encode(0x88, &[0x66, 0, 0, 0]),
            vec![0xbb, 0x88],
        ] {
            let mut script = keys();
            script[4].rx.extend(tail);
            let (mut wire, _, trace) = mock(script);
            // Exercise all ACKs without waiting ten seconds for the partial-tail case.
            for key in 1..=5 {
                wire.seq.next_send = Instant::now();
                let k = KEYS[(key - 1) as usize];
                wire.send(0x88, &[key, k[0], k[1], k[2]]).unwrap();
                let r = wire
                    .next(Instant::now() + Duration::from_secs(1))
                    .unwrap()
                    .unwrap();
                wire.seq.ack(key, Instant::now(), r.ingress).unwrap();
            }
            assert!(!wire.complete());
            assert!(wire.into_session().is_err());
            fs::remove_file(trace).unwrap();
        }
    }
    #[test]
    fn partial_frame_keeps_first_byte_timestamp_and_rejects_stale_ack() {
        let t = Instant::now();
        let mut d = TimedDecoder::default();
        let b = protocol::encode(0x88, &[0x11, 2, 0, 0x1b]);
        assert!(d.feed(&b[..3], t).unwrap().is_empty());
        assert!(d.has_partial());
        let out = d.feed(&b[3..], t + RETRY).unwrap();
        assert_eq!(out[0].ingress, t);
        assert!(!d.has_partial());
        let mut s = Sequence::new(t - Duration::from_millis(60));
        s.sent(t).unwrap();
        s.ack(1, t, t).unwrap();
        s.sent(t + Duration::from_millis(1)).unwrap();
        assert!(s.ack(2, t + RETRY, out[0].ingress).is_err());
    }
    #[test]
    fn nak_epochs_retry_spacing_and_no_post_ack2_retry() {
        let t = Instant::now();
        let mut s = Sequence::new(t);
        assert!(!s.can_send(t + Duration::from_millis(59)));
        s.sent(t + Duration::from_millis(60)).unwrap();
        assert!(!s.can_send(t + Duration::from_millis(159)));
        s.nak(t + Duration::from_millis(70), t + Duration::from_millis(70))
            .unwrap();
        assert_eq!(s.epoch, 1);
        assert!(!s.can_send(t + Duration::from_millis(169)));
        let x = t + Duration::from_millis(170);
        s.sent(x).unwrap();
        s.ack(1, x, x).unwrap();
        s.sent(x).unwrap();
        s.ack(2, x, x).unwrap();
        s.sent(x).unwrap();
        assert!(!s.can_send(x + RETRY));
        assert!(s.nak(x, x).is_err());
    }
    #[test]
    fn duplicate_out_of_order_nak_without_request_and_tx_cap_reject() {
        let t = Instant::now();
        let x = t + Duration::from_millis(60);
        let mut s = Sequence::new(t);
        assert!(s.nak(x, x).is_err());
        s.sent(x).unwrap();
        assert!(s.ack(2, x, x).is_err());
        s.ack(1, x, x).unwrap();
        assert!(s.ack(1, x, x).is_err());
        assert!(s.nak(x, x).is_err());
        s.tx_count = MAX_TX;
        assert!(!s.can_send(x));
        assert!(!s.can_send(t + WINDOW));
    }
    #[test]
    fn malformed_raw_data_never_authorizes_handover() {
        for bytes in [
            vec![1],
            vec![0xbb, 0x88, 0, 0, 0xbb],
            vec![0xbb, 0x88, 0xbd, 0x99],
            vec![0xbb, 0x88, 0xbd, 0xbb],
        ] {
            assert!(TimedDecoder::default()
                .feed(&bytes, Instant::now())
                .is_err());
        }
    }
    #[test]
    fn uncertain_key_write_halts_without_reconfiguration() {
        let (path, trace) = temp_file();
        let port = MockPort {
            steps: Rc::new(RefCell::new(VecDeque::new())),
            pending: VecDeque::new(),
            fail_write: true,
        };
        let mut wire = Wire::new(Box::new(port), trace);
        wire.seq.next_send = Instant::now();
        assert!(wire.handshake(&mut |_| {}).is_err());
        assert_eq!(wire.seq.tx_count, 1);
        assert!(wire.send(0x10, &[]).is_err());
        drop(wire);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn stock_allowlist_and_live_identity_are_exact() {
        let (d, m) = stock();
        validate_stock(&d, &m).unwrap();
        let mut changed = d.clone();
        changed.metadata.source_sha256 = "changed".into();
        assert!(validate_stock(&changed, &m).is_err());
        changed = d.clone();
        changed.data.pop();
        assert!(validate_stock(&changed, &m).is_err());
        changed = d.clone();
        changed.metadata.board = Some(2);
        assert!(validate_stock(&changed, &m).is_err());
        let boot = BootIdentity {
            slot: 31,
            version: [5, 0, 6, 1],
            board: 1,
            series: 0x23,
            unit: 0,
            serial: SERIAL,
        };
        validate_live(&boot, Some(&SERIAL)).unwrap();
        validate_live(&boot, None).unwrap();
        let mut wrong_board = boot.clone();
        wrong_board.board = 2;
        let mut wrong_series = boot.clone();
        wrong_series.series = 0x20;
        let mut wrong_unit = boot.clone();
        wrong_unit.unit = 1;
        let mut wrong_slot = boot.clone();
        wrong_slot.slot = 13;
        for other in [wrong_board, wrong_series, wrong_unit, wrong_slot] {
            assert!(validate_live(&other, Some(&SERIAL)).is_err());
            assert!(validate_live(&other, None).is_err());
        }
        let mut other = boot.clone();
        other.version[3] = 2;
        assert!(validate_live(&other, Some(&SERIAL)).is_err());
        assert!(validate_live(&other, None).is_err());
        other = boot.clone();
        other.serial[0] = 9;
        assert!(validate_live(&other, Some(&SERIAL)).is_err());
        validate_live(&other, None).unwrap();
        for invalid in [[0; 6], [0xff; 6]] {
            other.serial = invalid;
            assert!(validate_live(&other, None).is_err());
        }
    }
}
