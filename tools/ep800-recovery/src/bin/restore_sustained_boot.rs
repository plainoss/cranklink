//! Experimental stock-only EP800 attach restore; see docs/ep800-hardware-recovery.md.
//! Does not initialize/reset adapter power or select a protocol on attachment.
use anyhow::{ensure, Context, Result};
use cranklink::{
    device::Session,
    firmware::{Firmware, FirmwareKind, Version},
    platform,
    protocol::Frame,
};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

const D_SOURCE: &str = "35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543";
const D_DECODED: &str = "f2f2d18441b7651620a0aacfd4f9f31b2a97b6397bf0f03b2171ff9652cf2551";
const M_HASH: &str = "94986ebf8c21587a2b54b197f19a51c4118918bdca0aff6296a7727f7dfacc44";
const KEYS: [[u8; 4]; 5] = [
    [1, 0x31, 0x41, 0x59],
    [2, 0x26, 0x53, 0x58],
    [3, 0x97, 0x93, 0x23],
    [4, 0x84, 0x62, 0x64],
    [5, 0x33, 0x83, 0x27],
];

// Durable journal adapted from cli.rs::Journal.
// Every stage is durable before proceeding; flash itself remains the library writer.
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
            json!({"time_ms": platform::now_ms(), "stage": stage, "data": data})
        )?;
        self.0.sync_all()?;
        Ok(())
    }
}

// Strict framing consumes every raw byte, including split frames; labels are never proof.
#[derive(Default)]
struct StrictRx {
    active: bool,
    escaped: bool,
    data: Vec<u8>,
}
impl StrictRx {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Frame>> {
        let mut out = Vec::new();
        for &b in bytes {
            if !self.active {
                ensure!(b == 0xff || b == 0xbb, "unframed raw RX byte");
                if b == 0xbb {
                    self.active = true;
                    self.data.clear();
                }
                continue;
            }
            if b == 0xbb {
                ensure!(
                    !self.escaped && self.data.len() >= 2,
                    "empty/truncated raw RX frame"
                );
                ensure!(
                    self.data.iter().fold(0u8, |a, &b| a.wrapping_add(b)) == 0,
                    "raw RX FCS"
                );
                out.push(Frame {
                    control: self.data[0],
                    payload: self.data[1..self.data.len() - 1].to_vec(),
                });
                self.data.clear();
                self.active = false;
                continue;
            }
            if self.escaped {
                ensure!(matches!(b, 0x9b | 0x9d), "invalid raw escape");
                self.data.push(b ^ 0x20);
                self.escaped = false;
            } else if b == 0xbd {
                self.escaped = true;
            } else {
                self.data.push(b);
            }
            ensure!(self.data.len() <= 4096, "raw RX frame overflow");
        }
        Ok(out)
    }
    fn complete(&self) -> bool {
        !self.active && !self.escaped && self.data.is_empty()
    }
}
#[derive(Clone)]
struct LocalExchange {
    control: u8,
    payload: Vec<u8>,
    reply: Frame,
    tx_ms: u128,
    rx_ms: u128,
}
fn exact(e: &LocalExchange, c: u8, p: &[u8], rc: u8, rp: &[u8]) -> bool {
    e.control == c && e.payload == p && e.reply.control == rc && e.reply.payload == rp
}
fn validate_setup(v: &[LocalExchange]) -> Result<&'static str> {
    ensure!(
        v.len() >= 7
            && exact(&v[0], 0x10, &[], 0x30, &[0])
            && exact(&v[1], 0x1a, &[1], 0x3a, &[1, 0]),
        "missing reset/unlock setup"
    );
    let mut i = 2;
    let mut caps = false;
    let mut last_cap = None;
    loop {
        let e = v.get(i).context("no ready2 before setup")?;
        i += 1;
        if e.control == 0x1c {
            ensure!(
                e.payload.is_empty() && e.reply.control == 0x3c && e.reply.payload.len() == 2,
                "bad capacitor query"
            );
            caps = true;
            last_cap = Some(u16::from_le_bytes(e.reply.payload[..2].try_into().unwrap()));
            continue;
        }
        ensure!(
            e.control == 0x1a
                && e.payload == [2]
                && e.reply.control == 0x3a
                && e.reply.payload.len() == 2
                && e.reply.payload[0] == 2,
            "unexpected readiness sequence"
        );
        ensure!(
            !caps || last_cap.is_some_and(|c| c >= 2704 && c != 0xffff),
            "readiness without fresh adequate capacitor cache"
        );
        match e.reply.payload[1] {
            2 => break,
            3 | 4 => {}
            _ => anyhow::bail!("not ready/charging"),
        }
    }
    let tail = &v[i..];
    if !caps && tail.len() == 4 && exact(&tail[0], 0x1a, &[3], 0x3a, &[3, 0]) {
        ensure!(
            exact(&tail[1], 3, &[0x60], 0x23, &[0])
                && exact(&tail[2], 4, &[], 0x24, &[0xe0])
                && exact(&tail[3], 6, &[31], 0x26, &[0]),
            "historical routing mismatch"
        );
        ensure!(
            tail[1].tx_ms >= tail[0].rx_ms + 2000 && tail[2].tx_ms >= tail[1].rx_ms + 1000,
            "historical settling waits missing"
        );
        return Ok("historical");
    }
    ensure!(
        caps && (tail.len() == 6 || tail.len() == 7),
        "unsupported setup profile"
    );
    let fixed = [
        (3, vec![0x60], 0x23, vec![0]),
        (4, vec![], 0x24, vec![0xe0]),
        (6, vec![31], 0x26, vec![0]),
        (7, vec![0, 1, 0, 0, 0], 0x27, vec![0, 0]),
        (7, vec![5], 0x27, vec![5, 0]),
    ];
    for (e, (c, p, rc, rp)) in tail.iter().zip(&fixed) {
        ensure!(
            exact(e, *c, p, *rc, rp),
            "current profile routing/mask/framing mismatch"
        );
    }
    let stop = tail.len() == 7;
    if stop {
        ensure!(
            exact(&tail[5], 0x1a, &[6], 0x3a, &[6, 0]),
            "missing STOP6 ACK"
        );
    }
    ensure!(
        exact(tail.last().unwrap(), 0x1a, &[3], 0x3a, &[3, 0]),
        "missing finalON"
    );
    Ok(if stop {
        "current-stop6"
    } else {
        "current-normal"
    })
}
fn local_reply(c: u8, p: &[u8], f: &Frame) -> bool {
    match (c, p) {
        (0x10, []) => f.control == 0x30 && f.payload == [0],
        (0x1a, [1 | 3 | 6]) => f.control == 0x3a && f.payload == [p[0], 0],
        (0x1a, [2]) => {
            f.control == 0x3a
                && f.payload.len() == 2
                && f.payload[0] == 2
                && matches!(f.payload[1], 2 | 3 | 4)
        }
        (0x1c, []) => f.control == 0x3c && f.payload.len() == 2,
        (3, [0x60]) => f.control == 0x23 && f.payload == [0],
        (4, []) => f.control == 0x24 && f.payload == [0xe0],
        (6, [31]) => f.control == 0x26 && f.payload == [0],
        (7, [0, 1, 0, 0, 0]) => f.control == 0x27 && f.payload == [0, 0],
        (7, [5]) => f.control == 0x27 && f.payload == [5, 0],
        _ => false,
    }
}
#[derive(Clone)]
struct ExpectedEvent {
    name: &'static str,
    reason: &'static str,
    epoch: u64,
    key: Option<u8>,
    acks: Vec<u8>,
    expected: u8,
    progress: bool,
}
fn validate_evidence(text: &str, now_ms: u128) -> Result<Value> {
    use std::collections::VecDeque;
    let mut setup = Vec::<LocalExchange>::new();
    let mut local: Option<(u8, Vec<u8>, u128)> = None;
    let mut written = false;
    let mut rx = StrictRx::default();
    let mut summary = false;
    let mut last_ms = 0u128;
    let mut on_ms = None;
    let mut key_tx = 0u64;
    let mut epoch = 0u64;
    let mut naks = 0u64;
    let mut profile = None;
    let mut outstanding = false;
    let mut acks = Vec::<(u8, u128)>::new();
    let mut epoch_first_tx = None;
    let mut events = Vec::<Value>::new();
    let mut pending_events = VecDeque::<ExpectedEvent>::new();
    let mut records = 0usize;
    let mut last_key_tx = None;
    let mut first_key_tx = None;
    let mut initial_marker = false;
    let mut profile_marker: Option<String> = None;
    let mut nak_retry_after = 0u128;
    for line in text.lines() {
        ensure!(!line.trim().is_empty(), "empty proof record");
        let row: Value = serde_json::from_str(line).context("invalid proof JSON")?;
        records += 1;
        ensure!(!summary, "records after completed summary");
        if row["mode"] == "sustained-key-epochs" {
            ensure!(
                profile.is_some()
                    && local.is_none()
                    && !outstanding
                    && acks.len() == 5
                    && rx.complete()
                    && pending_events.is_empty(),
                "summary without complete raw/event proof"
            );
            ensure!(
                row["completed"] == true
                    && row["diagnostic_error"].is_null()
                    && row["outcome"] == "full-handshake"
                    && row["full_handshake"] == true
                    && row["progress_seen"] == true
                    && row["restored"] == false
                    && row["firmware_or_settings_writes"] == false
                    && row["error"].is_null(),
                "failed/forged summary"
            );
            ensure!(
                row["motor_tx_count"].as_u64() == Some(key_tx)
                    && row["final_epoch"].as_u64() == Some(epoch)
                    && row["ordered_key_acks"] == json!([1, 2, 3, 4, 5]),
                "summary counts/order disagree with wire"
            );
            ensure!(
                row["events"] == json!(events),
                "summary events differ from checked event log"
            );
            let final_events: Vec<Value> = events
                .iter()
                .filter(|e| e["event"] == "ack" && e["epoch"] == epoch)
                .map(|e| json!({"key":e["key"],"epoch":e["epoch"],"time_ms":e["time_ms"]}))
                .collect();
            ensure!(
                row["final_epoch_ack_events"] == json!(final_events)
                    && row["key_tx_count"].as_u64() == Some(key_tx)
                    && row["nak_count"].as_u64() == Some(naks),
                "compatibility proof summary mismatch"
            );
            ensure!(
                row["setup_profile"].as_str() == profile
                    && row["normal_vendor_prep"] == json!(profile == Some("current-normal"))
                    && row["historical_prep"] == json!(profile == Some("historical"))
                    && row["stop6_ack"] == json!(profile == Some("current-stop6")),
                "summary setup profile mismatch"
            );
            ensure!(
                row["window_ms"] == 10000 && row["retry_min_ms"] == 100,
                "summary bounds mismatch"
            );
            let finished = row["finished_at_ms"]
                .as_u64()
                .context("missing completion time")? as u128;
            ensure!(
                finished >= last_ms && finished <= now_ms,
                "invalid summary completion time"
            );
            last_ms = finished;
            summary = true;
            continue;
        }
        let direction = row["direction"].as_str().context("unknown proof record")?;
        let time = row["time_ms"].as_u64().context("missing record time")? as u128;
        ensure!(
            time >= last_ms && time <= now_ms,
            "nonmonotonic/future proof"
        );
        last_ms = time;
        if matches!(direction, "epoch-marker" | "sequence-event") {
            if direction == "epoch-marker" && !initial_marker {
                ensure!(
                    local.is_none()
                        && setup.len() >= 7
                        && row["event"] == "epoch-start"
                        && row["reason"] == "initial"
                        && row["epoch"] == 0
                        && row["key"].is_null()
                        && key_tx == 0,
                    "forged initial epoch marker"
                );
                profile = Some(validate_setup(&setup)?);
                ensure!(
                    profile_marker.as_deref() == profile,
                    "setup-profile marker disagrees with raw"
                );
                initial_marker = true;
                pending_events.push_back(ExpectedEvent {
                    name: "epoch-start",
                    reason: "initial",
                    epoch: 0,
                    key: None,
                    acks: vec![],
                    expected: 1,
                    progress: false,
                });
            }
            let e = pending_events
                .pop_front()
                .context("event without raw cause")?;
            ensure!(
                row["event"] == e.name
                    && row["reason"] == e.reason
                    && row["epoch"].as_u64() == Some(e.epoch)
                    && row["key"] == json!(e.key)
                    && row["ordered_key_acks"] == json!(e.acks)
                    && row["expected_key"] == e.expected
                    && row["progress_seen"] == e.progress,
                "event disagrees with raw state"
            );
            ensure!(
                (direction == "epoch-marker") == (e.name == "epoch-start"),
                "wrong event direction"
            );
            events.push(row);
            continue;
        }
        let bytes = hex::decode(row["bytes"].as_str().context("missing raw bytes")?)?;
        if direction == "rx-frame" {
            continue;
        } // Annotation never advances proof.
        if direction == "setup-profile" {
            ensure!(
                profile_marker.is_none() && key_tx == 0 && local.is_none(),
                "unexpected setup-profile marker"
            );
            let p = String::from_utf8(bytes)?;
            ensure!(
                matches!(
                    p.as_str(),
                    "historical" | "current-normal" | "current-stop6"
                ),
                "unknown profile marker"
            );
            profile_marker = Some(p);
            continue;
        }
        if direction == "cleanup-stop6-ack" {
            ensure!(
                profile.is_none()
                    && key_tx == 0
                    && on_ms.is_none()
                    && bytes == [0x3a, 6, 0]
                    && setup
                        .last()
                        .is_some_and(|e| exact(e, 0x1a, &[6], 0x3a, &[6, 0])),
                "cleanup label without raw STOP6 ACK"
            );
            continue;
        }
        let cleanup = matches!(direction, "cleanup-tx" | "cleanup-rx");
        if cleanup {
            ensure!(
                profile.is_none() && key_tx == 0 && on_ms.is_none(),
                "cleanup afterON/keys"
            );
        }
        let direction = match direction {
            "cleanup-tx" => "tx",
            "cleanup-rx" => "rx",
            x => x,
        };
        if matches!(
            direction,
            "ordinary-vendor-prep-no-stop6" | "stop6-prepared-condition"
        ) {
            ensure!(
                bytes.is_empty() && key_tx == 0 && local.is_none(),
                "invalid prep marker"
            );
            continue;
        }
        if direction == "tx-host-complete" {
            ensure!(
                bytes.is_empty() && (local.is_some() || outstanding) && !written,
                "unexpected host completion"
            );
            written = true;
            continue;
        }
        ensure!(
            matches!(direction, "tx" | "rx"),
            "unexpected proof direction"
        );
        if direction == "tx" {
            ensure!(
                rx.complete() && pending_events.is_empty(),
                "TX with incomplete frame/event"
            );
            if profile.is_none() {
                ensure!(
                    local.is_none() && !initial_marker,
                    "unacknowledged setup command"
                );
                let mut d = StrictRx::default();
                let f = d.feed(&bytes)?;
                ensure!(f.len() == 1 && d.complete(), "malformed setupTX");
                let f = &f[0];
                ensure!(
                    bytes == cranklink::protocol::encode(f.control, &f.payload),
                    "noncanonical setupTX"
                );
                ensure!(
                    matches!(f.control, 0x10 | 0x1a | 0x1c | 3 | 4 | 6 | 7),
                    "motorTX before valid setup/initial epoch marker"
                );
                ensure!(
                    !cleanup || (f.control == 0x1a && f.payload == [6]),
                    "unexpected cleanupTX"
                );
                local = Some((f.control, f.payload.clone(), time));
                written = cleanup;
            } else {
                ensure!(
                    initial_marker && acks.len() < 5,
                    "TX before epoch marker/after fullhandshake"
                );
                let key = acks.len() + 1;
                ensure!(time >= nak_retry_after, "retry too soon after NAK");
                if key_tx == 0 && profile != Some("historical") {
                    ensure!(time >= on_ms.unwrap_or(0) + 60, "initial key before60ms");
                }
                ensure!(
                    !outstanding || (key <= 2 && written),
                    "retry after ACK2/incomplete host write"
                );
                if outstanding {
                    ensure!(
                        time >= last_key_tx.unwrap_or(0) + 100,
                        "too-fast outstanding retry"
                    );
                }
                ensure!(
                    bytes == cranklink::protocol::encode(0x88, &KEYS[key - 1]),
                    "unexpected motorTX/setup change"
                );
                if first_key_tx.is_none() {
                    first_key_tx = Some(time);
                }
                ensure!(
                    time - first_key_tx.unwrap() <= 10000 && key_tx < 120,
                    "handshake window/TX cap exceeded"
                );
                if epoch_first_tx.is_none() {
                    epoch_first_tx = Some(time);
                }
                last_key_tx = Some(time);
                outstanding = true;
                written = false;
                key_tx += 1;
                pending_events.push_back(ExpectedEvent {
                    name: "key-tx",
                    reason: "fixed-key",
                    epoch,
                    key: Some(key as u8),
                    acks: acks.iter().map(|x| x.0).collect(),
                    expected: key as u8,
                    progress: acks.len() >= 2,
                });
            }
        } else {
            for f in rx.feed(&bytes)? {
                if f.control == 0x88 && f.payload == [0x70, 0, 0, 0x78] {
                    ensure!(on_ms.is_some(), "motor reply beforeON");
                    continue;
                }
                if let Some((c, p, t)) = local.take() {
                    ensure!(written && local_reply(c, &p, &f), "wrong local setupACK");
                    if c == 0x1a && p == [3] {
                        ensure!(on_ms.is_none(), "multipleON");
                        on_ms = Some(time);
                    }
                    setup.push(LocalExchange {
                        control: c,
                        payload: p,
                        reply: f,
                        tx_ms: t,
                        rx_ms: time,
                    });
                    written = false;
                    continue;
                }
                ensure!(
                    profile.is_some() && outstanding && written && f.control == 0x88,
                    "unsolicited motor response"
                );
                if f.payload == [0x12, 0, 0, 0x1a] {
                    ensure!(acks.len() < 2, "NAK after ACK2 invalidates proof");
                    let key = acks.len() + 1;
                    pending_events.push_back(ExpectedEvent {
                        name: "nak",
                        reason: "exact-nak",
                        epoch,
                        key: Some(key as u8),
                        acks: acks.iter().map(|x| x.0).collect(),
                        expected: key as u8,
                        progress: false,
                    });
                    nak_retry_after = time + 100;
                    naks += 1;
                    epoch += 1;
                    acks.clear();
                    epoch_first_tx = None;
                    outstanding = false;
                    written = false;
                    pending_events.push_back(ExpectedEvent {
                        name: "epoch-start",
                        reason: "nak",
                        epoch,
                        key: None,
                        acks: vec![],
                        expected: 1,
                        progress: false,
                    });
                    continue;
                }
                let key = acks.len() + 1;
                ensure!(
                    f.payload == [0x11, key as u8, 0, key as u8 + 0x19],
                    "malformed/outoforder/duplicateACK"
                );
                ensure!(
                    time - first_key_tx.unwrap() <= 10000,
                    "ACK after bounded window"
                );
                acks.push((key as u8, time));
                outstanding = false;
                written = false;
                pending_events.push_back(ExpectedEvent {
                    name: "ack",
                    reason: "exact-ack",
                    epoch,
                    key: Some(key as u8),
                    acks: acks.iter().map(|x| x.0).collect(),
                    expected: if key == 5 { 5 } else { key as u8 + 1 },
                    progress: acks.len() >= 2,
                });
            }
        }
    }
    ensure!(
        summary && rx.complete() && acks.len() == 5 && on_ms.is_some(),
        "no completed sameepoch fiveACK proof; NO device opened"
    );
    let first = epoch_first_tx.context("missing finalepoch firstTX")?;
    let key5 = acks[4].1;
    ensure!(
        now_ms - first <= 20000 && now_ms - key5 <= 20000 && now_ms - last_ms <= 20000,
        "full finalepoch proof stale"
    );
    Ok(
        json!({"records":records,"target":31,"ordered_acks":5,"final_epoch":epoch,"negative_ack_epochs":naks,"key_tx_count":key_tx,"power_on_ack_ms":on_ms,"setup_profile":profile,"first_final_epoch_tx_ms":first,"last_key5_ack_ms":key5,"last_raw_ms":last_ms,"checked_at_ms":now_ms,"raw_only":true}),
    )
}
fn validate_stock(d: &Firmware, m: &Firmware) -> Result<()> {
    ensure!(
        d.metadata.source_sha256 == D_SOURCE && d.metadata.decoded_sha256 == D_DECODED,
        "original D source/decoded hash mismatch"
    );
    ensure!(
        m.metadata.source_sha256 == M_HASH && m.metadata.decoded_sha256 == M_HASH,
        "original M source/decoded hash mismatch"
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
        "exact stock metadata mismatch"
    );
    d.validate_companion(m)
}
fn validate_live(boot: &cranklink::device::BootIdentity, serial: &[u8; 6]) -> Result<()> {
    ensure!(
        boot.serial == *serial
            && boot.version == [5, 0, 6, 1]
            && boot.board == 1
            && boot.series == 0x23
            && boot.unit == 0
            && boot.slot == 31,
        "fresh motor identity mismatch; NO write"
    );
    Ok(())
}

fn validate_mtime(path: &Path) -> Result<()> {
    ensure!(
        fs::metadata(path)?.modified()?.elapsed()? <= Duration::from_secs(20),
        "evidence file is stale/future"
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 7 && args[0] == "--execute",
        "Usage: restore_sustained_boot --execute <fresh-sustained-wire.jsonl> <NEW-journal.jsonl> <NEW-trace.jsonl> <stock-D.dat> <stock-M.dat> <expected-12-hex-serial>; STOCK D WRITE");
    let evidence_path = PathBuf::from(&args[1]);
    let journal_path = PathBuf::from(&args[2]);
    let trace_path = PathBuf::from(&args[3]);
    let d = Firmware::load(Path::new(&args[4]))?;
    let m = Firmware::load(Path::new(&args[5]))?;
    let serial: [u8; 6] = hex::decode(args[6].to_str().context("serial must be UTF-8")?)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("serial must contain exactly 12 hex digits"))?;
    validate_stock(&d, &m)?;
    validate_mtime(&evidence_path)?;
    let evidence_text = fs::read_to_string(&evidence_path)?;
    let evidence = validate_evidence(&evidence_text, platform::now_ms() as u128)?;
    ensure!(
        journal_path != trace_path && journal_path != evidence_path && trace_path != evidence_path,
        "evidence, journal and trace paths must differ"
    );
    let mut journal = Journal::new(&journal_path)?;
    let trace = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&trace_path)
        .context("trace must be a new file")?;
    trace.sync_all()?;
    drop(trace);
    journal.record("inputs", json!({"d":d.metadata,"m":m.metadata,"evidence_path":evidence_path,"evidence_validation":evidence,
        "scope":"attach to observed open bootloader; no adapter initialize/power/protocol/boot-entry commands"}))?;

    // Session::open configures FTDI UART; it does not perform Session::initialize.
    // Root must keep exclusive ownership and must not power-cycle after the evidence.
    ensure!(
        fs::read_to_string(&evidence_path)? == evidence_text,
        "evidence changed after validation"
    );
    validate_mtime(&evidence_path)?;
    validate_evidence(&evidence_text, platform::now_ms() as u128)?;
    let mut session = Session::open("usb", Some(&trace_path))?;
    session.target(31)?; // adapter routing only; does not initialize/reset motor.
    let boot = session.boot_identity()?;
    journal.record("fresh-boot-identity", json!(boot))?;
    validate_live(&boot, &serial)?;
    d.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))?;
    journal.record("destructive-start", json!({"d":d.metadata,"boot":boot,
        "note":"existing Session.flash_dcas re-reads identity, preserves serial/model, then erases/programs stock D; M/settings not written"}))?;
    let receipt = match session.flash_dcas(&d.data, &boot, |done, total| {
        if done == total || done % 8192 == 0 {
            eprintln!("Stock D {done}/{total}");
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
    journal.record("post-verification-pending", json!({"required":"operator must verify D4.2.0.0/M4.1.11.0, original serial, cargo identity and saved settings"}))?;
    println!("Stock transfer finish/reset acknowledged. ORIGINAL STATE IS NOT YET VERIFIED; perform application/cargo/settings readback.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SERIAL: [u8; 6] = [1, 2, 3, 4, 5, 6];
    struct Fixture {
        rows: Vec<Value>,
        events: Vec<Value>,
        t: u64,
        acks: Vec<u8>,
        epoch: u64,
        count: u64,
    }
    impl Fixture {
        fn raw(&mut self, d: &str, c: u8, p: &[u8]) {
            self.rows.push(json!({"direction":d,"bytes":hex::encode(cranklink::protocol::encode(c,p)),"time_ms":self.t}));
            self.t += 1;
        }
        fn complete(&mut self) {
            self.rows
                .push(json!({"direction":"tx-host-complete","bytes":"","time_ms":self.t}));
            self.t += 1;
        }
        fn local(&mut self, c: u8, p: &[u8], rc: u8, rp: &[u8]) {
            self.raw("tx", c, p);
            self.complete();
            self.raw("rx", rc, rp);
        }
        fn event(&mut self, name: &str, reason: &str, key: Option<u8>, expected: u8) {
            let row = json!({"direction":if name=="epoch-start"{"epoch-marker"}else{"sequence-event"},"event":name,"reason":reason,"key":key,"epoch":self.epoch,"ordered_key_acks":self.acks,"expected_key":expected,"progress_seen":self.acks.len()>=2,"time_ms":self.t});
            self.t += 1;
            self.rows.push(row.clone());
            self.events.push(row);
        }
        fn key(&mut self, k: usize, nak: bool, retry: bool) {
            self.raw("tx", 0x88, &KEYS[k - 1]);
            self.complete();
            self.count += 1;
            self.event("key-tx", "fixed-key", Some(k as u8), k as u8);
            if retry {
                self.t += 100;
                self.raw("tx", 0x88, &KEYS[k - 1]);
                self.complete();
                self.count += 1;
                self.event("key-tx", "fixed-key", Some(k as u8), k as u8);
            }
            if nak {
                self.raw("rx", 0x88, &[0x12, 0, 0, 0x1a]);
                self.event("nak", "exact-nak", Some(k as u8), k as u8);
                self.epoch += 1;
                self.acks.clear();
                self.event("epoch-start", "nak", None, 1);
                self.t += 100;
            } else {
                self.raw("rx", 0x88, &[0x11, k as u8, 0, k as u8 + 0x19]);
                self.acks.push(k as u8);
                self.event(
                    "ack",
                    "exact-ack",
                    Some(k as u8),
                    if k == 5 { 5 } else { k as u8 + 1 },
                );
            }
        }
    }
    fn fixture(profile: &str, nak: bool, retry: bool) -> Vec<Value> {
        let mut f = Fixture {
            rows: vec![],
            events: vec![],
            t: 1000,
            acks: vec![],
            epoch: 0,
            count: 0,
        };
        f.local(0x10, &[], 0x30, &[0]);
        f.local(0x1a, &[1], 0x3a, &[1, 0]);
        if profile != "historical" {
            f.local(0x1c, &[], 0x3c, &[0xb8, 0x0b]);
        }
        f.local(0x1a, &[2], 0x3a, &[2, 2]);
        if profile == "historical" {
            f.local(0x1a, &[3], 0x3a, &[3, 0]);
            f.t += 2000;
        }
        f.local(3, &[0x60], 0x23, &[0]);
        f.t += 1000;
        f.local(4, &[], 0x24, &[0xe0]);
        f.local(6, &[31], 0x26, &[0]);
        if profile != "historical" {
            f.local(7, &[0, 1, 0, 0, 0], 0x27, &[0, 0]);
            f.local(7, &[5], 0x27, &[5, 0]);
            if profile == "current-stop6" {
                f.raw("cleanup-tx", 0x1a, &[6]);
                f.raw("cleanup-rx", 0x3a, &[6, 0]);
                f.rows
                    .push(json!({"direction":"cleanup-stop6-ack","bytes":"3a0600","time_ms":f.t}));
                f.t += 1;
            }
        }
        f.rows
            .push(json!({"direction":"setup-profile","bytes":hex::encode(profile),"time_ms":f.t}));
        f.t += 1;
        if profile != "historical" {
            f.local(0x1a, &[3], 0x3a, &[3, 0]);
            f.t += 60;
        }
        f.event("epoch-start", "initial", None, 1);
        if nak {
            f.key(1, false, false);
            f.key(2, true, false);
        }
        for k in 1..=5 {
            f.key(k, false, retry && k == 2);
        }
        let finals: Vec<Value> = f
            .events
            .iter()
            .filter(|e| e["event"] == "ack" && e["epoch"] == f.epoch)
            .map(|e| json!({"key":e["key"],"epoch":e["epoch"],"time_ms":e["time_ms"]}))
            .collect();
        f.rows.push(json!({"completed":true,"diagnostic_error":null,"key_tx_count":f.count,"nak_count":if nak{1}else{0},"final_epoch_ack_events":finals,"mode":"sustained-key-epochs","outcome":"full-handshake","error":null,"final_epoch":f.epoch,"ordered_key_acks":f.acks,"progress_seen":true,"full_handshake":true,"motor_tx_count":f.count,"normal_vendor_prep":profile=="current-normal","historical_prep":profile=="historical","stop6_ack":profile=="current-stop6","setup_profile":profile,"finished_at_ms":f.t,"window_ms":10000,"retry_min_ms":100,"events":f.events,"firmware_or_settings_writes":false,"restored":false}));
        f.rows
    }
    fn text(f: &[Value]) -> String {
        f.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn good(f: &[Value]) -> bool {
        validate_evidence(&text(f), 6000).is_ok()
    }
    fn matches(r: &Value, d: &str, c: u8, p: &[u8]) -> bool {
        r["direction"] == d && r["bytes"] == hex::encode(cranklink::protocol::encode(c, p))
    }
    #[test]
    fn complete_three_profiles_nak_epochs_and_repeat() {
        for p in ["historical", "current-normal", "current-stop6"] {
            for n in [false, true] {
                for r in [false, true] {
                    let f = fixture(p, n, r);
                    assert!(
                        good(&f),
                        "{p}/{n}/{r}: {:?}",
                        validate_evidence(&text(&f), 6000)
                    );
                }
            }
        }
    }
    #[test]
    fn partial_mixed_epoch_forged_summary_missing_nak() {
        let mut f = fixture("historical", false, false);
        f.retain(|r| !matches(r, "rx", 0x88, &[0x11, 5, 0, 0x1e]));
        assert!(!good(&f));
        assert!(!good(&[fixture("historical", false, false).pop().unwrap()]));
        let mut f = fixture("historical", true, false);
        f.retain(|r| !matches(r, "rx", 0x88, &[0x12, 0, 0, 0x1a]));
        assert!(!good(&f));
        let mut f = fixture("historical", false, false);
        let i = f.iter().position(|r| r["event"] == "ack").unwrap();
        f[i]["epoch"] = json!(1);
        assert!(!good(&f));
        let mut f = fixture("historical", false, false);
        let n = f.len() - 1;
        f[n]["events"] = json!([]);
        assert!(!good(&f));
    }
    #[test]
    fn nak_after_two_early_duplicate_ack_and_protocol_change() {
        for p in [[0x12, 0, 0, 0x1a], [0x11, 1, 0, 0x1a]] {
            let mut f = fixture("historical", false, false);
            let i = f
                .iter()
                .position(|r| matches(r, "rx", 0x88, &[0x11, 3, 0, 0x1c]))
                .unwrap();
            f[i]["bytes"] = json!(hex::encode(cranklink::protocol::encode(0x88, &p)));
            assert!(!good(&f));
        }
        let mut f = fixture("historical", false, false);
        let ack2 = f
            .iter()
            .find(|r| matches(r, "rx", 0x88, &[0x11, 2, 0, 0x1b]))
            .unwrap()
            .clone();
        let i = f
            .iter()
            .position(|r| matches(r, "rx", 0x88, &[0x11, 1, 0, 0x1a]))
            .unwrap();
        let mut b = hex::decode(f[i]["bytes"].as_str().unwrap()).unwrap();
        b.extend(hex::decode(ack2["bytes"].as_str().unwrap()).unwrap());
        f[i]["bytes"] = json!(hex::encode(b));
        assert!(!good(&f));
        let mut f = fixture("historical", false, false);
        let i = f
            .iter()
            .position(|r| matches(r, "tx", 0x88, &KEYS[2]))
            .unwrap();
        f[i]["bytes"] = json!(hex::encode(cranklink::protocol::encode(3, &[0x60])));
        assert!(!good(&f));
    }
    #[test]
    fn malformed_raw_no_on_wrong_mask_stale_and_tx_after_summary() {
        for bad in ["bb881105001e", "01", "bb881105001e00bb"] {
            let mut f = fixture("historical", false, false);
            let i = f
                .iter()
                .position(|r| matches(r, "rx", 0x88, &[0x11, 5, 0, 0x1e]))
                .unwrap();
            f[i]["bytes"] = json!(bad);
            assert!(!good(&f));
        }
        let mut f = fixture("historical", false, false);
        f.retain(|r| !matches(r, "rx", 0x3a, &[3, 0]));
        assert!(!good(&f));
        let mut f = fixture("current-normal", false, false);
        let i = f
            .iter()
            .position(|r| matches(r, "tx", 7, &[0, 1, 0, 0, 0]))
            .unwrap();
        f[i]["bytes"] = json!(hex::encode(cranklink::protocol::encode(
            7,
            &[0, 2, 0, 0, 0]
        )));
        assert!(!good(&f));
        let f = fixture("historical", false, false);
        assert!(validate_evidence(&text(&f), 30000).is_err());
        assert!(validate_evidence(&text(&f), 900).is_err());
        let mut f = fixture("historical", false, false);
        f.push(json!({"direction":"tx","bytes":hex::encode(cranklink::protocol::encode(0x10,&[])),"time_ms":5500}));
        assert!(!good(&f));
    }
    #[test]
    fn wrong_live_identity() {
        let mut b = cranklink::device::BootIdentity {
            slot: 31,
            version: [5, 0, 6, 1],
            board: 1,
            series: 0x23,
            unit: 0,
            serial: SERIAL,
        };
        validate_live(&b, &SERIAL).unwrap();
        b.serial[0] ^= 1;
        assert!(validate_live(&b, &SERIAL).is_err());
        b.serial = SERIAL;
        b.board = 2;
        assert!(validate_live(&b, &SERIAL).is_err());
        b.board = 1;
        b.version[0] = 6;
        assert!(validate_live(&b, &SERIAL).is_err());
    }
    #[test]
    fn stock_allowlist_rejects_changed_hash_or_metadata() {
        use cranklink::firmware::FirmwareMetadata;
        // Synthetic bytes and metadata only: no proprietary firmware fixture.
        // Real execution gets metadata solely from Firmware::load.
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
        let d = Firmware {
            metadata,
            data: vec![0; 157996],
        };
        let mut m = d.clone();
        m.metadata.kind = FirmwareKind::Renesas;
        m.metadata.version = Version([4, 1, 11, 0]);
        m.metadata.source_sha256 = M_HASH.into();
        m.metadata.decoded_sha256 = M_HASH.into();
        validate_stock(&d, &m).unwrap();
        let mut changed = d.clone();
        changed.metadata.source_sha256 = "changed".into();
        assert!(validate_stock(&changed, &m).is_err());
        changed = d.clone();
        changed.metadata.board = Some(2);
        assert!(validate_stock(&changed, &m).is_err());
        changed = d.clone();
        changed.data.pop();
        assert!(validate_stock(&changed, &m).is_err());
        m.metadata.decoded_sha256 = "changed".into();
        assert!(validate_stock(&d, &m).is_err());
    }
    #[test]
    fn stale_or_future_file_and_queued_nak_or_stale_ack() {
        let p = std::env::temp_dir().join(format!(
            "restore-sustained-mtime-{}-{}.jsonl",
            std::process::id(),
            platform::now_ms()
        ));
        let f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&p)
            .unwrap();
        assert!(validate_mtime(&p).is_ok());
        f.set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
            .unwrap();
        assert!(validate_mtime(&p).is_err());
        f.set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() + Duration::from_secs(100)),
        )
        .unwrap();
        assert!(validate_mtime(&p).is_err());
        drop(f);
        fs::remove_file(p).unwrap();
        let mut f = fixture("historical", false, false);
        let i = f
            .iter()
            .position(|r| matches(r, "rx", 0x88, &[0x11, 1, 0, 0x1a]))
            .unwrap();
        let mut b = hex::decode(f[i]["bytes"].as_str().unwrap()).unwrap();
        b.extend(cranklink::protocol::encode(0x88, &[0x12, 0, 0, 0x1a]));
        f[i]["bytes"] = json!(hex::encode(b));
        assert!(!good(&f));
        let mut f = fixture("historical", true, false);
        let i = f
            .iter()
            .position(|r| matches(r, "rx", 0x88, &[0x12, 0, 0, 0x1a]))
            .unwrap();
        let mut b = hex::decode(f[i]["bytes"].as_str().unwrap()).unwrap();
        b.extend(cranklink::protocol::encode(0x88, &[0x11, 1, 0, 0x1a]));
        f[i]["bytes"] = json!(hex::encode(b));
        assert!(!good(&f));
    }
}
