//! Bounded sustained fixed-key epochs. No firmware/settings writes.
use anyhow::{bail, ensure, Context, Result};
use cranklink::{
    platform::{self, Port},
    protocol::{self, Decoder, Frame},
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{File, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};
struct Received {
    frame: Frame,
    ingress: Instant,
}
#[derive(Default)]
struct TimedDecoder {
    inner: Decoder,
    active: bool,
    has_data: bool,
    start: Option<Instant>,
}
impl TimedDecoder {
    fn has_partial(&self) -> bool {
        self.active && self.has_data
    }
    fn feed(&mut self, bytes: &[u8], ingress: Instant) -> Vec<Result<Received, String>> {
        let mut out = Vec::new();
        for &byte in bytes {
            let frame_start = self.start.unwrap_or(ingress);
            let decoded = self.inner.feed(&[byte]);
            if byte == 0xbb {
                if !self.active {
                    self.active = true;
                    self.has_data = false;
                    self.start = Some(ingress);
                } else if self.has_data {
                    self.active = false;
                    self.has_data = false;
                    self.start = None;
                } else {
                    self.start = Some(ingress);
                }
            } else if self.active {
                self.has_data = true;
            }
            for item in decoded {
                match item {
                    Ok(frame) => out.push(Ok(Received {
                        frame,
                        ingress: frame_start,
                    })),
                    Err(e) => {
                        self.active = false;
                        self.has_data = false;
                        self.start = None;
                        out.push(Err(e));
                    }
                }
            }
        }
        out
    }
}
struct Wire {
    port: Box<dyn Port>,
    decoder: TimedDecoder,
    queue: VecDeque<Received>,
    trace: File,
    epoch: Instant,
    seq: Sequence,
    on_ack: Option<Instant>,
}
impl Wire {
    fn log(&mut self, direction: &str, bytes: &[u8]) -> Result<()> {
        writeln!(
            self.trace,
            "{}",
            serde_json::json!({"time_ms":platform::now_ms(),"elapsed_us":self.epoch.elapsed().as_micros(),"direction":direction,"bytes":hex::encode(bytes)})
        )?;
        self.trace.flush()?;
        Ok(())
    }
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        ensure!(
            control == 0x88 || self.seq.tx_count == 0,
            "no local/power/protocol commands after first motor key"
        );
        let allowed = match control {
            0x10 | 4 => payload.is_empty(),
            0x1a => matches!(payload, [1] | [2] | [3] | [4] | [6]),
            3 => payload == [0x20] || payload == [0x60],
            0x1c => payload.is_empty(),
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
        ensure!(allowed, "fixed read diagnostic allowlist rejected command");
        if control == 0x88 {
            self.seq.sent(Instant::now())?;
        }
        let bytes = protocol::encode(control, payload);
        self.log("tx", &bytes)?;
        self.port.set_timeout(Duration::from_millis(20))?;
        self.port
            .write_all(&bytes)
            .context("write outcome uncertain; no retry or cleanup")?;
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
                writeln!(
                    self.trace,
                    "{}",
                    serde_json::json!({"time_ms":platform::now_ms(),"elapsed_us":self.epoch.elapsed().as_micros(),"direction":"rx-frame","bytes":hex::encode(bytes),"rx_ingress_elapsed_us":received.ingress.duration_since(self.epoch).as_micros()})
                )?;
                self.trace.flush()?;
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
                    for f in self.decoder.feed(&bytes[..n], ingress) {
                        self.queue
                            .push_back(f.map_err(|e| anyhow::anyhow!("decode: {e}"))?);
                    }
                    ensure!(self.queue.len() <= 2048, "RX queue overflow");
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(e).context("read failed; no retry or cleanup"),
            }
        }
    }
    // Cleanup must still reach USB when a trace write has failed.
    fn stop_discharge(&mut self) -> Result<()> {
        let bytes = protocol::encode(0x1a, &[6]);
        let _ = self.log("cleanup-tx", &bytes);
        self.port.set_timeout(Duration::from_millis(20))?;
        self.port
            .write_all(&bytes)
            .context("STOP6 write uncertain")?;
        #[cfg(not(windows))]
        self.port.flush()?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            while let Some(f) = self.queue.pop_front() {
                if f.frame.control == 0x3a && f.frame.payload == [6, 0] {
                    let _ = self.log("cleanup-stop6-ack", &[0x3a, 6, 0]);
                    return Ok(());
                }
            }
            ensure!(Instant::now() < deadline, "STOP6 acknowledgement deadline");
            self.port.set_timeout(Duration::from_millis(2))?;
            let mut bytes = [0u8; 4096];
            match self.port.read(&mut bytes) {
                Ok(n) if n > 0 => {
                    let ingress = Instant::now();
                    let _ = self.log("cleanup-rx", &bytes[..n]);
                    for f in self.decoder.feed(&bytes[..n], ingress) {
                        if let Ok(f) = f {
                            self.queue.push_back(f);
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
    fn cap(&mut self) -> Result<u16> {
        self.send(0x1c, &[])?;
        let f = self
            .next(Instant::now() + Duration::from_secs(3))?
            .context("no capacitor reply")?
            .frame;
        ensure!(
            f.control == 0x3c && f.payload.len() == 2,
            "wrong capacitor reply"
        );
        Ok(u16::from_le_bytes([f.payload[0], f.payload[1]]))
    }
    fn ack(&mut self, control: u8, payload: &[u8], response: u8, expected: &[u8]) -> Result<()> {
        self.send(control, payload)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while let Some(received) = self.next(deadline)? {
            let f = received.frame;
            if f.control == response && f.payload == expected {
                return Ok(());
            }
            if f.control == 0x88 && f.payload == [0x70, 0, 0, 0x78] {
                continue;
            }
            bail!(
                "unexpected adapter reply {:02x}:{}",
                f.control,
                hex::encode(f.payload)
            );
        }
        bail!("adapter acknowledgment deadline expired")
    }
}

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
        ensure!(self.can_send(now), "TX timing/state/cap guard rejected");
        self.tx_count += 1;
        self.last_tx = Some(now);
        self.outstanding = true;
        self.next_send = now + RETRY;
        Ok(())
    }
    fn ack(&mut self, key: u8, now: Instant, ingress: Instant) -> Result<()> {
        ensure!(
            self.last_tx.is_some_and(|tx| ingress >= tx),
            "ACK predates outstanding TX; preserve state"
        );
        ensure!(
            self.outstanding && key == self.expected,
            "unexpected/duplicate ACK{key}, expected{}",
            self.expected
        );
        self.acks.push(key);
        if key >= 2 {
            self.progress = true;
        }
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
            "NAK predates outstanding TX; preserve state"
        );
        ensure!(!self.progress, "NAK after ACK2; preserve state");
        ensure!(
            self.outstanding,
            "NAK without outstanding key; preserve state"
        );
        self.epoch += 1;
        self.expected = 1;
        self.acks.clear();
        self.outstanding = false;
        self.next_send = now + RETRY;
        Ok(())
    }
}
fn finish_eligible(seq: &Sequence, pending: usize, partial: bool) -> bool {
    seq.full && pending == 0 && !partial
}
fn event(
    w: &mut Wire,
    events: &mut Vec<serde_json::Value>,
    name: &str,
    key: Option<u8>,
    reason: &str,
) -> Result<()> {
    let row = serde_json::json!({"direction":if name=="epoch-start"{"epoch-marker"}else{"sequence-event"},"event":name,"epoch":w.seq.epoch,"key":key,"reason":reason,"expected_key":w.seq.expected,"ordered_key_acks":w.seq.acks,"progress_seen":w.seq.progress,"elapsed_after_on_us":w.on_ack.context("missing ON timestamp")?.elapsed().as_micros(),"elapsed_in_window_us":w.seq.start.elapsed().as_micros(),"elapsed_us":w.epoch.elapsed().as_micros(),"time_ms":platform::now_ms()});
    writeln!(w.trace, "{row}")?;
    w.trace.flush()?;
    events.push(row);
    Ok(())
}
fn run() -> Result<()> {
    let mut execute = false;
    let mut normal_prep = false;
    let mut historical_prep = false;
    let mut trace = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--execute" => execute = true,
            "--normal-prep" => normal_prep = true,
            "--historical-prep" => historical_prep = true,
            "--trace" => trace = Some(PathBuf::from(args.next().context("new trace needed")?)),
            _ => bail!("unknown argument {a}"),
        }
    }
    ensure!(
        !(normal_prep && historical_prep),
        "--normal-prep and --historical-prep are mutually exclusive"
    );
    if !execute {
        println!("NOT EXECUTED. --execute --trace NEW_FILE [--normal-prep | --historical-prep]. Historical profile: reset/unlock/1A2 readiness/ON/wait2s/update-master/wait1s/readback/target31; no capacitor/mask/mode/STOP6 commands on success;10sec begins after post-ON setup and first key immediate. Current stable setup; first fixed key1 at60ms,10sec window,120 TX cap. Same outstanding key retries >=100ms only before ACK2. Exact ACK advances immediately; only exact NAK before ACK2 resets local epoch. After ACK2 only ACK-gated3..5, no retries/reset. Stop at ACK5. No identity, firmware/settings, motorreset or automatic restore.");
        return Ok(());
    }
    let trace = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(trace.context("trace required")?)?;
    let mut w = Wire {
        port: platform::open_port("usb")?,
        decoder: TimedDecoder::default(),
        queue: VecDeque::new(),
        trace,
        epoch: Instant::now(),
        seq: Sequence::new(Instant::now()),
        on_ack: None,
    };
    let mut caps = Vec::new();
    let setup = (|| -> Result<()> {
        w.ack(0x10, &[], 0x30, &[0])?;
        std::thread::sleep(Duration::from_secs(3));
        w.ack(0x1a, &[1], 0x3a, &[1, 0])?;
        let limit = Instant::now() + Duration::from_secs(90);
        if historical_prep {
            loop {
                ensure!(Instant::now() < limit, "readiness deadline");
                w.send(0x1a, &[2])?;
                let f = w
                    .next(Instant::now() + Duration::from_secs(3))?
                    .context("no state")?
                    .frame;
                ensure!(
                    f.control == 0x3a && f.payload.len() == 2 && f.payload[0] == 2,
                    "wrong state"
                );
                if f.payload[1] == 2 {
                    break;
                }
                ensure!(matches!(f.payload[1], 3 | 4), "state not ready/charging");
                std::thread::sleep(Duration::from_secs(3));
            }
            w.ack(0x1a, &[3], 0x3a, &[3, 0])?;
            w.on_ack = Some(Instant::now());
            std::thread::sleep(Duration::from_secs(2));
        } else {
            loop {
                ensure!(Instant::now() < limit, "readiness deadline");
                let raw = w.cap()?;
                caps.push(raw);
                if raw != 0xffff && raw >= 2704 {
                    w.send(0x1a, &[2])?;
                    let f = w
                        .next(Instant::now() + Duration::from_secs(3))?
                        .context("no state")?
                        .frame;
                    ensure!(
                        f.control == 0x3a && f.payload.len() == 2 && f.payload[0] == 2,
                        "wrong state"
                    );
                    if f.payload[1] == 2 {
                        break;
                    }
                    ensure!(matches!(f.payload[1], 3 | 4), "state not ready/charging");
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        w.ack(3, &[0x60], 0x23, &[0])?;
        std::thread::sleep(Duration::from_secs(1));
        w.send(4, &[])?;
        let f = w
            .next(Instant::now() + Duration::from_secs(3))?
            .context("no mode")?
            .frame;
        ensure!(
            f.control == 0x24 && f.payload.len() == 1 && f.payload[0] & 0xe0 == 0xe0,
            "wrong mode"
        );
        w.ack(6, &[31], 0x26, &[0])?;
        if !historical_prep {
            w.ack(7, &[0, 1, 0, 0, 0], 0x27, &[0, 0])?;
            w.ack(7, &[5], 0x27, &[5, 0])?;
        }
        Ok(())
    })();
    if setup.is_err() || (!normal_prep && !historical_prep) {
        let stop = w.stop_discharge();
        ensure!(
            stop.is_ok(),
            "STOP6 unconfirmed; no ON: {stop:?}; setup: {setup:?}"
        );
    }
    setup?;
    let setup_profile = if historical_prep {
        "historical"
    } else if normal_prep {
        "current-normal"
    } else {
        "current-stop6"
    };
    w.log("setup-profile", setup_profile.as_bytes())?;
    if !historical_prep {
        w.ack(0x1a, &[3], 0x3a, &[3, 0])?;
        w.on_ack = Some(Instant::now());
    }
    let on_ack = w.on_ack.context("missing ON acknowledgement")?;
    let start = if historical_prep {
        Instant::now()
    } else {
        on_ack
    };
    w.seq = Sequence::new(start);
    if historical_prep {
        w.seq.next_send = start;
    }
    let mut events = Vec::new();
    let mut counts = BTreeMap::<String, u64>::new();
    event(&mut w, &mut events, "epoch-start", None, "initial")?;
    let observation = (|| -> Result<&'static str> {
        loop {
            let now = Instant::now();
            if finish_eligible(&w.seq, w.queue.len(), w.decoder.has_partial()) {
                return Ok("full-handshake");
            }
            if now >= w.seq.start + WINDOW {
                return Ok(if w.seq.progress {
                    "timeout-after-progress"
                } else {
                    "window-expired"
                });
            }
            if w.seq.tx_count >= MAX_TX {
                return Ok("motor-tx-cap");
            }
            if w.queue.is_empty() && !w.decoder.has_partial() && w.seq.can_send(now) {
                let key = w.seq.expected;
                let k = KEYS[(key - 1) as usize];
                w.send(0x88, &[key, k[0], k[1], k[2]])?;
                event(&mut w, &mut events, "key-tx", Some(key), "fixed-key")?;
            }
            let mut deadline = w.seq.start + WINDOW;
            if !w.decoder.has_partial() && !(w.seq.progress && w.seq.outstanding) {
                deadline = deadline.min(w.seq.next_send);
            }
            if let Some(received) = w.next(deadline)? {
                let ingress = received.ingress;
                let f = received.frame;
                let mut b = vec![f.control];
                b.extend_from_slice(&f.payload);
                *counts.entry(hex::encode(&b)).or_default() += 1;
                if f.control == 0x88 && f.payload == [0x70, 0, 0, 0x78] {
                    continue;
                }
                ensure!(
                    ingress < w.seq.start + WINDOW,
                    "response arrived after observation window; preserve state"
                );
                ensure!(
                    f.control == 0x88 && f.payload.len() == 4,
                    "unexpected response {:02x}:{}",
                    f.control,
                    hex::encode(&f.payload)
                );
                if f.payload[0] == 0x11 {
                    let key = f.payload[1];
                    ensure!(
                        (1..=5).contains(&key) && f.payload == [0x11, key, 0, 0x19 + key],
                        "malformed ACK"
                    );
                    w.seq.ack(key, Instant::now(), ingress)?;
                    event(&mut w, &mut events, "ack", Some(key), "exact-ack")?;
                } else if f.payload == [0x12, 0, 0, 0x1a] {
                    let expected = w.seq.expected;
                    event(&mut w, &mut events, "nak", Some(expected), "exact-nak")?;
                    w.seq.nak(Instant::now(), ingress)?;
                    event(&mut w, &mut events, "epoch-start", None, "nak")?;
                } else {
                    bail!(
                        "unknown/malformed response {}; preserve state",
                        hex::encode(f.payload)
                    );
                }
            }
        }
    })();
    let success = matches!(observation, Ok("full-handshake"))
        && finish_eligible(&w.seq, w.queue.len(), w.decoder.has_partial());
    let outcome = observation.as_ref().copied().unwrap_or("halted-error");
    let error = observation.as_ref().err().map(|e| format!("{e:#}"));
    let final_epoch_ack_events: Vec<_> = events
        .iter()
        .filter(|e| e["event"] == "ack" && e["epoch"] == w.seq.epoch)
        .map(|e| serde_json::json!({"key":e["key"],"epoch":e["epoch"],"time_ms":e["time_ms"]}))
        .collect();
    let nak_count = events.iter().filter(|e| e["event"] == "nak").count();
    let summary = serde_json::json!({"mode":"sustained-key-epochs","completed":success,"diagnostic_error":error,"key_tx_count":w.seq.tx_count,"nak_count":nak_count,"final_epoch_ack_events":final_epoch_ack_events,"setup_profile":setup_profile,"historical_prep":historical_prep,"outcome":outcome,"error":error,"final_epoch":w.seq.epoch,"ordered_key_acks":w.seq.acks,"progress_seen":w.seq.progress,"full_handshake":success,"motor_tx_count":w.seq.tx_count,"normal_vendor_prep":normal_prep,"stop6_ack":!normal_prep && !historical_prep,"initial_cap_samples":caps,"finished_at_ms":platform::now_ms(),"window_ms":10000,"retry_min_ms":100,"events":events,"parsed_frame_counts":counts,"firmware_or_settings_writes":false,"restored":false,"rail_waveform_measured":false});
    writeln!(w.trace, "{summary}")?;
    w.trace.sync_all()?;
    println!("{summary}");
    observation?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Stopped: {e:#}. No automatic retry/reset/power/followup after observation.");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_nak_spacing() {
        let t = Instant::now();
        let mut s = Sequence::new(t);
        assert!(!s.can_send(t));
        s.sent(t + Duration::from_millis(60)).unwrap();
        assert!(!s.can_send(t + Duration::from_millis(159)));
        s.nak(t + Duration::from_millis(70), t + Duration::from_millis(70))
            .unwrap();
        assert_eq!(s.epoch, 1);
        assert!(!s.can_send(t + Duration::from_millis(169)));
        assert!(s.can_send(t + Duration::from_millis(170)));
    }
    #[test]
    fn progress_no_retry_or_restart() {
        let t = Instant::now() + Duration::from_millis(60);
        let mut s = Sequence::new(t - Duration::from_millis(60));
        s.sent(t).unwrap();
        s.ack(1, t, t).unwrap();
        s.sent(t).unwrap();
        s.ack(2, t, t).unwrap();
        s.sent(t).unwrap();
        assert!(!s.can_send(t + RETRY));
        assert!(s.nak(t, t).is_err());
        assert_eq!(s.expected, 3);
    }
    #[test]
    fn duplicate_ack_and_caps() {
        let t = Instant::now();
        let mut s = Sequence::new(t);
        let x = t + Duration::from_millis(60);
        s.sent(x).unwrap();
        s.ack(1, x, x).unwrap();
        assert!(s.ack(1, x, x).is_err());
        assert!(!s.can_send(t + WINDOW));
        s.tx_count = MAX_TX;
        assert!(!s.can_send(x));
    }
    #[test]
    fn exact_five_finish() {
        let t = Instant::now();
        let mut s = Sequence::new(t);
        let x = t + Duration::from_millis(60);
        for k in 1..=5 {
            s.sent(x).unwrap();
            s.ack(k, x, x).unwrap();
        }
        assert!(s.full);
        assert_eq!(s.acks, [1, 2, 3, 4, 5]);
        assert_eq!(s.epoch, 0);
        assert!(!s.can_send(x + RETRY));
    }
    #[test]
    fn stale_ack1_after_nak_same_block() {
        let t = Instant::now();
        let mut seq = Sequence::new(t);
        let sent = t + Duration::from_millis(60);
        seq.sent(sent).unwrap();
        let rx = t + Duration::from_millis(70);
        let mut d = TimedDecoder::default();
        let mut b = protocol::encode(0x88, &[0x12, 0, 0, 0x1a]);
        b.extend(protocol::encode(0x88, &[0x11, 1, 0, 0x1a]));
        let mut q: VecDeque<_> = d.feed(&b, rx).into_iter().map(Result::unwrap).collect();
        let nak = q.pop_front().unwrap();
        seq.nak(rx, nak.ingress).unwrap();
        let stale = q.pop_front().unwrap();
        assert!(seq.ack(1, rx, stale.ingress).is_err());
        seq.sent(rx + RETRY).unwrap();
        assert!(seq.ack(1, rx + RETRY, stale.ingress).is_err());
    }
    #[test]
    fn queued_nak_after_ack1_cannot_advance() {
        let t = Instant::now();
        let mut seq = Sequence::new(t);
        let x = t + Duration::from_millis(60);
        seq.sent(x).unwrap();
        seq.ack(1, x, x).unwrap();
        // Existing pending frames prevent next-key transmission; a NAK without a sent key2 halts.
        let pending = 1;
        assert!(!(pending == 0 && seq.can_send(x)));
        assert!(seq.nak(x, x).is_err());
        assert_eq!(seq.tx_count, 1);
    }
    #[test]
    fn ack2_buffered_before_tx2_rejected() {
        let t = Instant::now();
        let mut seq = Sequence::new(t);
        let x = t + Duration::from_millis(60);
        seq.sent(x).unwrap();
        seq.ack(1, x, x).unwrap();
        seq.sent(x + Duration::from_millis(1)).unwrap();
        assert!(seq.ack(2, x + Duration::from_millis(2), x).is_err());
    }
    #[test]
    fn partial_frame_retains_earliest_chunk_stamp() {
        let t = Instant::now();
        let mut d = TimedDecoder::default();
        let b = protocol::encode(0x88, &[0x11, 2, 0, 0x1b]);
        assert!(d.feed(&b[..3], t).is_empty());
        assert!(d.has_partial());
        let out = d.feed(&b[3..], t + RETRY);
        let f = out.into_iter().next().unwrap().unwrap();
        assert_eq!(f.ingress, t);
        assert!(!d.has_partial());
    }
    #[test]
    fn ack5_plus_nak_blocks_full_proof() {
        let t = Instant::now();
        let mut seq = Sequence::new(t);
        let x = t + Duration::from_millis(60);
        for key in 1..=4 {
            seq.sent(x).unwrap();
            seq.ack(key, x, x).unwrap();
        }
        seq.sent(x).unwrap();
        let mut d = TimedDecoder::default();
        let mut b = protocol::encode(0x88, &[0x11, 5, 0, 0x1e]);
        b.extend(protocol::encode(0x88, &[0x12, 0, 0, 0x1a]));
        let mut q: VecDeque<_> = d.feed(&b, x).into_iter().map(Result::unwrap).collect();
        let ack = q.pop_front().unwrap();
        seq.ack(5, x, ack.ingress).unwrap();
        assert!(!finish_eligible(&seq, q.len(), d.has_partial()));
        let nak = q.pop_front().unwrap();
        assert!(seq.nak(x, nak.ingress).is_err());
    }
    #[test]
    fn ack5_plus_unexpected_blocks_full_proof() {
        let t = Instant::now();
        let mut seq = Sequence::new(t);
        let x = t + Duration::from_millis(60);
        for key in 1..=5 {
            seq.sent(x).unwrap();
            seq.ack(key, x, x).unwrap();
        }
        assert!(!finish_eligible(&seq, 1, false));
        assert!(!finish_eligible(&seq, 0, true));
        assert!(seq.ack(1, x, x).is_err());
    }
}
