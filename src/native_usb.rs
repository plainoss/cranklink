//! Direct FTDI USB transport for SM-PCE02, including custom VID/PID devices
//! without a virtual COM driver. No libusb, vendor DLL, or async runtime needed.
use super::Port;
use anyhow::{bail, Context, Result};
use nusb::{
    descriptors::TransferType,
    transfer::{Bulk, ControlOut, ControlType, In, Out, Recipient},
    Endpoint, MaybeFuture,
};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{mpsc, Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const VID: u16 = 0x1e44;
const PID: u16 = 0x7151;
const PACKET_SIZE: usize = 64;
const TRANSFER_SIZE: usize = PACKET_SIZE * 64;
const CONTROL_TIMEOUT: Duration = Duration::from_millis(500);
// Same reset/purge, 8N1, no flow control, 2 Mbaud, RTS, latency sequence as WebUSB.
const CONTROLS: &[(u8, u16)] = &[
    (0, 0),
    (0, 1),
    (0, 2),
    (4, 8),
    (2, 0),
    (3, 1),
    (1, 0x0202),
    (9, 2),
];

pub(super) fn devices() -> Result<Vec<nusb::DeviceInfo>> {
    Ok(nusb::list_devices()
        .wait()
        .context("enumerate USB devices")?
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect())
}

pub(super) fn open() -> Result<Box<dyn Port>> {
    let mut devices = devices()?;
    if devices.len() != 1 {
        bail!(
            "--port usb requires exactly one SM-PCE02 1E44:7151; found {}",
            devices.len()
        );
    }
    open_device(devices.remove(0))
}

pub(super) fn open_device(info: nusb::DeviceInfo) -> Result<Box<dyn Port>> {
    anyhow::ensure!(
        info.vendor_id() == VID && info.product_id() == PID,
        "only SM-PCE02 1E44:7151 is supported"
    );
    if !matches!(info.device_version(), 0x0400 | 0x0600 | 0x0A00 | 0x1000) {
        bail!(
            "unsupported FTDI revision {:04X}; no motor command sent",
            info.device_version()
        );
    }
    let device = info.open().wait().context(access_help())?;
    let config = device
        .active_configuration()
        .context("SM-PCE02 must already have an active USB configuration")?;
    let interface_desc = config
        .interface_alt_settings()
        .find(|i| i.interface_number() == 0 && i.alternate_setting() == 0 && i.class() == 255)
        .context("SM-PCE02 lacks the supported vendor interface 0/alternate 0")?;
    if config.num_interfaces() != 1 {
        bail!("only single-interface FTDI adapters are supported");
    }
    let endpoints: Vec<_> = interface_desc.endpoints().collect();
    for address in [0x81, 0x02] {
        if !endpoints.iter().any(|e| {
            e.address() == address
                && e.transfer_type() == TransferType::Bulk
                && e.max_packet_size() == PACKET_SIZE
        }) {
            bail!("SM-PCE02 lacks audited bulk endpoint {address:02X} with 64-byte packets");
        }
    }
    // Do not detach drivers or reset/reconfigure the device automatically.
    let interface = device.claim_interface(0).wait().context(access_help())?;
    let input = interface
        .endpoint::<Bulk, In>(0x81)
        .context("open USB IN endpoint")?;
    let output = interface
        .endpoint::<Bulk, Out>(0x02)
        .context("open USB OUT endpoint")?;
    for &(request, value) in CONTROLS {
        interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request,
                    value,
                    index: 0,
                    data: &[],
                },
                CONTROL_TIMEOUT,
            )
            .wait()
            .with_context(|| format!("FTDI UART setup request {request:02X}/{value:04X}"))?;
    }
    let receiver = ReceivePump::start(UsbInput(input))?;
    Ok(Box::new(FtdiPort::new(UsbEndpoints { receiver, output })))
}

fn access_help() -> &'static str {
    if cfg!(target_os = "windows") {
        "open/claim SM-PCE02 USB: close browser, E-TUBE and COM tools; use the adapter's FTDI COM driver with --port COMn"
    } else if cfg!(target_os = "linux") {
        "open/claim SM-PCE02 USB: close its other owner; permission denied requires a udev rule granting access to USB 1e44:7151"
    } else {
        "open/claim SM-PCE02 USB: close browser, E-TUBE and serial tools using this adapter"
    }
}

/// Backend is separated so queue/timeouts/poison behavior is testable without USB.
trait Transfers {
    /// Serial bytes with FTDI status removed; None means timeout.
    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
    /// Return only after a completion, or fail without retrying an uncertain write.
    fn send(&mut self, data: &[u8], timeout: Duration) -> io::Result<usize>;
}
struct UsbEndpoints {
    receiver: ReceivePump,
    output: Endpoint<Bulk, Out>,
}
impl Transfers for UsbEndpoints {
    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        self.receiver.receive(timeout)
    }
    fn send(&mut self, data: &[u8], timeout: Duration) -> io::Result<usize> {
        self.receiver.check()?;
        // Never submit another OUT if one has an uncertain completion.
        if self.output.pending() != 0 {
            return Err(io::Error::other("previous USB write still pending"));
        }
        self.output.submit(data.into());
        let Some(done) = self.output.wait_next_complete(timeout) else {
            self.output.cancel_all();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "USB write timed out",
            ));
        };
        done.status?;
        Ok(done.actual_len)
    }
}

const RECEIVE_LIMIT: usize = 1024 * 1024;
const PUMP_WAIT: Duration = Duration::from_millis(50);
const STOP_WAIT: Duration = Duration::from_millis(200);

trait RawInput: Send + 'static {
    /// Keep one submitted transfer across wait timeouts, then immediately
    /// resubmit after each completion, even while the protocol engine sleeps.
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>>;
    fn cancel(&mut self);
}
struct UsbInput(Endpoint<Bulk, In>);
impl RawInput for UsbInput {
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        if self.0.pending() == 0 {
            self.0.submit(self.0.allocate(TRANSFER_SIZE));
        }
        let Some(done) = self.0.wait_next_complete(timeout) else {
            return Ok(None);
        };
        done.status?;
        Ok(Some(done.buffer.into_vec()))
    }
    fn cancel(&mut self) {
        self.0.cancel_all();
    }
}
#[derive(Default)]
struct ReceiveState {
    bytes: VecDeque<u8>,
    error: Option<(io::ErrorKind, String)>,
    stopped: bool,
}
type SharedReceive = Arc<(Mutex<ReceiveState>, Condvar)>;
struct ReceivePump {
    shared: SharedReceive,
    done: mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}
impl ReceivePump {
    fn start(input: impl RawInput) -> io::Result<Self> {
        let shared: SharedReceive = Arc::default();
        let worker_shared = shared.clone();
        let (finished, done) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("SM-PCE02 USB receive".into())
            .spawn(move || {
                run_pump(input, worker_shared);
                // Endpoint destruction/cancellation finishes before signaling completion.
                let _ = finished.send(());
            })?;
        Ok(Self {
            shared,
            done,
            thread: Some(thread),
        })
    }
    fn check(&self) -> io::Result<()> {
        check_state(&self.shared.0.lock().unwrap())
    }
    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let start = Instant::now();
        let (mutex, changed) = &*self.shared;
        let mut state = mutex.lock().unwrap();
        loop {
            check_state(&state)?;
            if !state.bytes.is_empty() {
                let count = state.bytes.len().min(TRANSFER_SIZE);
                return Ok(Some(state.bytes.drain(..count).collect()));
            }
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Ok(None);
            }
            state = changed.wait_timeout(state, remaining).unwrap().0;
        }
    }
}
fn check_state(state: &ReceiveState) -> io::Result<()> {
    if let Some((kind, message)) = &state.error {
        return Err(io::Error::new(*kind, message.clone()));
    }
    if state.stopped {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "USB receive pump stopped",
        ));
    }
    Ok(())
}
fn run_pump(mut input: impl RawInput, shared: SharedReceive) {
    let (mutex, changed) = &*shared;
    loop {
        if mutex.lock().unwrap().stopped {
            break;
        }
        let data = match input
            .next(PUMP_WAIT)
            .and_then(|raw| raw.map(|r| strip_status(&r)).transpose())
        {
            Ok(Some(data)) => data,
            Ok(None) => continue,
            Err(error) => {
                let mut state = mutex.lock().unwrap();
                state.error = Some((error.kind(), error.to_string()));
                changed.notify_all();
                break;
            }
        };
        // Status-only traffic must not consume queue capacity or wake readers.
        if data.is_empty() {
            continue;
        }
        let mut state = mutex.lock().unwrap();
        if state.stopped {
            break;
        }
        if data.len() > RECEIVE_LIMIT.saturating_sub(state.bytes.len()) {
            state.error = Some((
                io::ErrorKind::InvalidData,
                "USB receive queue exceeded 1 MiB; session stopped".into(),
            ));
            changed.notify_all();
            break;
        }
        state.bytes.extend(data);
        changed.notify_all();
    }
    input.cancel();
}
impl Drop for ReceivePump {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().stopped = true;
        self.shared.1.notify_all();
        // Native transfer cancellation is asynchronous. Do not let a wedged OS
        // backend turn protocol failure/shutdown into an unbounded join.
        if self.done.recv_timeout(STOP_WAIT).is_ok() {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
        // On timeout, dropping JoinHandle detaches a worker that owns all its
        // endpoint/state resources. No caller can reuse it or dispatch more OUT.
    }
}

fn strip_status(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut serial = Vec::with_capacity(bytes.len());
    for packet in bytes.chunks(PACKET_SIZE) {
        if packet.len() < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated FTDI status header",
            ));
        }
        if packet[1] & 0x1e != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("FTDI UART line error {:02X}", packet[1]),
            ));
        }
        serial.extend_from_slice(&packet[2..]);
    }
    Ok(serial)
}
struct FtdiPort<T> {
    transfers: T,
    serial: VecDeque<u8>,
    timeout: Duration,
    poisoned: Option<String>,
}
impl<T: Transfers> FtdiPort<T> {
    fn new(transfers: T) -> Self {
        Self {
            transfers,
            serial: VecDeque::new(),
            timeout: Duration::from_millis(100),
            poisoned: None,
        }
    }
    fn check(&self) -> io::Result<()> {
        match &self.poisoned {
            Some(reason) => Err(io::Error::other(reason.clone())),
            None => Ok(()),
        }
    }
    fn poison(&mut self, error: io::Error, uncertain: bool) -> io::Error {
        let reason = if uncertain {
            format!("USB write outcome uncertain: {error}; session stopped; reconcile motor before another command")
        } else {
            format!("USB receive failed: {error}; session stopped")
        };
        self.poisoned = Some(reason.clone());
        io::Error::new(error.kind(), reason)
    }
}
impl<T: Transfers> Read for FtdiPort<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let start = Instant::now();
        while self.serial.is_empty() {
            let remaining = self.timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "USB read timed out",
                ));
            }
            let data = self
                .transfers
                .receive(remaining)
                .map_err(|e| self.poison(e, false))?;
            let Some(data) = data else {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "USB read timed out",
                ));
            };
            self.serial.extend(data);
        }
        let count = bytes.len().min(self.serial.len());
        for byte in &mut bytes[..count] {
            *byte = self.serial.pop_front().unwrap();
        }
        Ok(count)
    }
}
impl<T: Transfers> Write for FtdiPort<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check()?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = self
            .transfers
            .send(bytes, self.timeout)
            .map_err(|e| self.poison(e, true))?;
        if count != bytes.len() {
            return Err(self.poison(
                io::Error::other(format!("incomplete transfer {count}/{}", bytes.len())),
                true,
            ));
        }
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()
    }
}
impl<T: Transfers> Port for FtdiPort<T> {
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.timeout = timeout;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Mock {
        reads: VecDeque<io::Result<Option<Vec<u8>>>>,
        writes: usize,
        result: Option<io::Result<usize>>,
        timeouts: Vec<Duration>,
    }
    impl Transfers for Mock {
        fn receive(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
            self.timeouts.push(timeout);
            self.reads.pop_front().unwrap_or(Ok(None))
        }
        fn send(&mut self, _: &[u8], _: Duration) -> io::Result<usize> {
            self.writes += 1;
            self.result.take().unwrap_or(Ok(2))
        }
    }
    #[test]
    fn status_is_removed_per_packet_and_serial_escape_bytes_are_preserved() {
        let mut raw = vec![1, 0x60];
        raw.extend(0..62);
        raw.extend([1, 0x60, 0xbb, 0xbd, 0xff, 0]);
        let mut expected: Vec<_> = (0..62).collect();
        expected.extend([0xbb, 0xbd, 0xff, 0]);
        assert_eq!(strip_status(&raw).unwrap(), expected);
        assert!(strip_status(&[1, 0x60]).unwrap().is_empty());
        assert!(strip_status(&[]).unwrap().is_empty());
        assert!(strip_status(&[1]).is_err());
        for error in [2, 4, 8, 16] {
            assert!(strip_status(&[1, 0x60 | error, 0xbb]).is_err());
        }
    }
    #[test]
    fn short_reads_preserve_queue_and_timeout_does_not_poison_late_data() {
        let mut mock = Mock::default();
        mock.reads
            .extend([Ok(None), Ok(Some(vec![])), Ok(Some(vec![1, 2, 3]))]);
        let mut port = FtdiPort::new(mock);
        let mut bytes = [0; 2];
        assert_eq!(
            port.read(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(port.read(&mut bytes).unwrap(), 2);
        assert_eq!(bytes, [1, 2]);
        assert_eq!(port.read(&mut bytes).unwrap(), 1);
        assert_eq!(bytes[0], 3);
        assert_eq!(port.transfers.timeouts.len(), 3);
        assert!(port.poisoned.is_none());
    }
    #[test]
    fn failed_timeout_or_partial_write_never_dispatches_a_retry() {
        for result in [
            Err(io::Error::new(io::ErrorKind::TimedOut, "deadline")),
            Err(io::Error::other("endpoint stalled")),
            Ok(1),
        ] {
            let mut port = FtdiPort::new(Mock {
                result: Some(result),
                ..Mock::default()
            });
            assert!(port
                .write_all(&[1, 2])
                .unwrap_err()
                .to_string()
                .contains("uncertain"));
            assert!(port.write_all(&[3, 4]).is_err());
            assert!(port.flush().is_err());
            assert_eq!(port.transfers.writes, 1);
        }
    }
    #[test]
    fn line_error_poison_prevents_writes_after_corrupted_receive() {
        let mut mock = Mock::default();
        mock.reads.push_back(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "FTDI UART line error",
        )));
        let mut port = FtdiPort::new(mock);
        assert!(port.read(&mut [0; 16]).is_err());
        assert!(port.write_all(&[1, 2]).is_err());
        assert_eq!(port.transfers.writes, 0);
    }
    struct ChannelInput {
        incoming: mpsc::Receiver<io::Result<Option<Vec<u8>>>>,
        polls: Arc<std::sync::atomic::AtomicUsize>,
        cancels: Arc<std::sync::atomic::AtomicUsize>,
        ignore_timeout: bool,
    }
    impl RawInput for ChannelInput {
        fn next(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
            use std::sync::atomic::Ordering;
            self.polls.fetch_add(1, Ordering::SeqCst);
            if self.ignore_timeout {
                return self.incoming.recv().unwrap_or(Ok(None));
            }
            match self.incoming.recv_timeout(timeout) {
                Ok(result) => result,
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "mock disconnected",
                )),
            }
        }
        fn cancel(&mut self) {
            self.cancels
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    fn channel_input(
        ignore_timeout: bool,
    ) -> (ChannelInput, mpsc::Sender<io::Result<Option<Vec<u8>>>>) {
        let (sender, incoming) = mpsc::channel();
        (
            ChannelInput {
                incoming,
                polls: Arc::default(),
                cancels: Arc::default(),
                ignore_timeout,
            },
            sender,
        )
    }
    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now();
        while !condition() {
            assert!(
                deadline.elapsed() < Duration::from_secs(2),
                "background pump did not progress"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn receive_pump_runs_without_protocol_reads_and_preserves_late_bytes_after_timeout() {
        use std::sync::atomic::Ordering;
        let (input, sender) = channel_input(false);
        let polls = input.polls.clone();
        let cancels = input.cancels.clone();
        let mut pump = ReceivePump::start(input).unwrap();
        sender
            .send(Ok(Some(vec![1, 0x60, 0xbb, 0xbd, 0xff])))
            .unwrap();
        // The protocol engine can sleep: USB completion is already drained and
        // another IN is awaited before any receive call occurs here.
        until(|| {
            pump.shared.0.lock().unwrap().bytes.len() == 3 && polls.load(Ordering::SeqCst) >= 2
        });
        assert_eq!(
            pump.receive(Duration::from_millis(10)).unwrap(),
            Some(vec![0xbb, 0xbd, 0xff])
        );
        assert_eq!(pump.receive(Duration::from_millis(5)).unwrap(), None);
        assert_eq!(cancels.load(Ordering::SeqCst), 0);
        sender.send(Ok(Some(vec![1, 0x60, 7]))).unwrap();
        assert_eq!(
            pump.receive(Duration::from_millis(100)).unwrap(),
            Some(vec![7])
        );
        drop(pump);
        assert_eq!(cancels.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn receive_pump_discards_status_only_traffic_and_enforces_queue_limit() {
        use std::sync::atomic::Ordering;
        let (input, sender) = channel_input(false);
        let polls = input.polls.clone();
        let pump = ReceivePump::start(input).unwrap();
        for _ in 0..1000 {
            sender.send(Ok(Some(vec![1, 0x60]))).unwrap();
        }
        until(|| polls.load(Ordering::SeqCst) >= 1001);
        assert!(pump.shared.0.lock().unwrap().bytes.is_empty());
        let mut transfer = Vec::new();
        for _ in 0..64 {
            transfer.extend([1, 0x60]);
            transfer.extend([7; 62]);
        }
        for _ in 0..266 {
            let _ = sender.send(Ok(Some(transfer.clone())));
        }
        until(|| pump.check().is_err());
        assert!(pump
            .check()
            .unwrap_err()
            .to_string()
            .contains("exceeded 1 MiB"));
        assert!(pump.shared.0.lock().unwrap().bytes.len() <= RECEIVE_LIMIT);
    }
    #[test]
    fn receive_pump_error_wakes_a_waiting_reader() {
        let (input, sender) = channel_input(false);
        let mut pump = ReceivePump::start(input).unwrap();
        let reader = thread::spawn(move || pump.receive(Duration::from_secs(2)));
        sender
            .send(Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "USB unplugged",
            )))
            .unwrap();
        assert!(reader
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("USB unplugged"));
    }
    #[test]
    fn receive_pump_drop_is_bounded_even_if_backend_ignores_timeout() {
        use std::sync::atomic::Ordering;
        let (input, sender) = channel_input(true);
        let polls = input.polls.clone();
        let cancels = input.cancels.clone();
        let pump = ReceivePump::start(input).unwrap();
        until(|| polls.load(Ordering::SeqCst) >= 1);
        let start = Instant::now();
        drop(pump);
        assert!(start.elapsed() < Duration::from_secs(1));
        // The safely detached worker owns its endpoint. Once the simulated OS
        // releases the wait, it observes stop and cancels without resubmission.
        sender.send(Ok(None)).unwrap();
        until(|| cancels.load(Ordering::SeqCst) == 1);
        assert_eq!(polls.load(Ordering::SeqCst), 1);
    }
}
