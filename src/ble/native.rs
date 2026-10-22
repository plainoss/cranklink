//! Native GATT boundary. The Tokio runtime and bounded notification pump belong
//! to one connection; no async work survives close/drop.
use super::*;
use btleplug::{
    api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Manager, Peripheral},
};
use futures_util::StreamExt;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle, time::timeout};
use uuid::Uuid;

fn runtime() -> Result<Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}
async fn bounded<T>(
    future: impl Future<Output = std::result::Result<T, btleplug::Error>>,
) -> Result<T> {
    timeout(Duration::from_secs(10), future)
        .await
        .context("Bluetooth operation timed out")?
        .context("Bluetooth operation failed (check OS permission and pairing)")
}
async fn discover(seconds: u64) -> Result<Vec<(PeripheralInfo, Peripheral)>> {
    let manager = bounded(Manager::new()).await?;
    let adapters = bounded(manager.adapters()).await?;
    ensure!(!adapters.is_empty(), "no Bluetooth adapters available");
    let mut started = Vec::new();
    let result: Result<Vec<(PeripheralInfo, Peripheral)>> = async {
        for adapter in &adapters {
            bounded(adapter.start_scan(ScanFilter::default())).await?;
            started.push(adapter);
        }
        tokio::time::sleep(Duration::from_secs(seconds)).await;
        let mut found = Vec::new();
        for adapter in &adapters {
            for p in bounded(adapter.peripherals()).await? {
                if let Some(props) = bounded(p.properties()).await? {
                    found.push((
                        PeripheralInfo {
                            id: p.id().to_string(),
                            name: props.local_name,
                            rssi: props.rssi,
                            etube_service_advertised: props
                                .services
                                .contains(&Uuid::parse_str(SERVICE)?),
                        },
                        p,
                    ));
                }
            }
        }
        Ok(found)
    }
    .await;
    let mut stop_error = None;
    for a in started {
        if let Err(e) = bounded(a.stop_scan()).await {
            stop_error = Some(e);
        }
    }
    let found = result?;
    if let Some(e) = stop_error {
        return Err(e).context("stop Bluetooth discovery");
    }
    Ok(found)
}
pub(super) fn scan(seconds: u64, all: bool) -> Result<Vec<PeripheralInfo>> {
    let mut result: Vec<_> = runtime()?
        .block_on(discover(seconds))?
        .into_iter()
        .map(|(info, _)| info)
        .filter(|i| all || i.etube_service_advertised)
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}
pub(super) struct NativeGatt {
    rt: Runtime,
    peripheral: Peripheral,
    chars: BTreeMap<u16, btleplug::api::Characteristic>,
    rx: mpsc::Receiver<Notification>,
    pump: JoinHandle<()>,
    failed: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
    credential: Option<String>,
    closed: bool,
}
impl NativeGatt {
    pub fn open(id: &str, credential: Option<String>) -> Result<Self> {
        let rt = runtime()?;
        let mut matches: Vec<_> = rt
            .block_on(discover(5))?
            .into_iter()
            .filter(|(i, _)| i.id == id)
            .collect();
        ensure!(
            matches.len() == 1,
            "expected one BLE peripheral with ID {id}; found {} (run ble scan)",
            matches.len()
        );
        let (_, peripheral) = matches.remove(0);
        let setup = rt.block_on(async {
            bounded(peripheral.connect()).await?;
            bounded(peripheral.discover_services()).await?;
            let service = Uuid::parse_str(SERVICE)?;
            let chars: BTreeMap<_, _> = peripheral
                .characteristics()
                .into_iter()
                .filter(|c| c.service_uuid == service)
                .filter_map(|c| {
                    let s = c.uuid.to_string();
                    (s.ends_with("-5348-494d-414e-4f5f424c4500") && s.starts_with("0000"))
                        .then(|| u16::from_str_radix(&s[4..8], 16).ok().map(|id| (id, c)))
                        .flatten()
                })
                .collect();
            ensure!(
                chars.contains_key(&PCE_COMMAND) && chars.contains_key(&FREE_COMMAND),
                "selected peripheral has no E-TUBE configuration service"
            );
            let mut notifications = bounded(peripheral.notifications()).await?;
            let (tx, rx) = mpsc::channel(256);
            let failed = Arc::new(AtomicBool::new(false));
            let failed_task = failed.clone();
            let overflow = Arc::new(AtomicBool::new(false));
            let overflow_task = overflow.clone();
            let ids: BTreeMap<_, _> = chars.iter().map(|(id, c)| (c.uuid, *id)).collect();
            let pump = tokio::spawn(async move {
                while let Some(n) = notifications.next().await {
                    if let Some(&id) = ids.get(&n.uuid) {
                        if n.value.len() > 20
                            || tx
                                .try_send(Notification {
                                    characteristic: id,
                                    value: n.value,
                                })
                                .is_err()
                        {
                            overflow_task.store(true, Ordering::Release);
                            failed_task.store(true, Ordering::Release);
                            return;
                        }
                    }
                }
                failed_task.store(true, Ordering::Release);
            });
            Ok((chars, rx, pump, failed, overflow))
        });
        let (chars, rx, pump, failed, overflow) = match setup {
            Ok(x) => x,
            Err(e) => {
                let _ = rt.block_on(bounded(peripheral.disconnect()));
                return Err(e);
            }
        };
        Ok(Self {
            rt,
            peripheral,
            chars,
            rx,
            pump,
            failed,
            overflow,
            credential,
            closed: false,
        })
    }
    fn ready(&self) -> Result<()> {
        ensure!(
            !self.closed && !self.failed.load(Ordering::Acquire),
            "Bluetooth disconnected or receive queue failed; reconnect"
        );
        Ok(())
    }
    fn characteristic(&self, id: u16) -> Result<btleplug::api::Characteristic> {
        self.ready()?;
        self.chars
            .get(&id)
            .cloned()
            .context("required E-TUBE GATT characteristic is absent")
    }
    fn finish<T>(&mut self, result: Result<T>) -> Result<T> {
        if result.is_err() {
            self.failed.store(true, Ordering::Release);
            let _ = self.close();
        }
        result
    }
}
impl Gatt for NativeGatt {
    fn characteristics(&self) -> Vec<CharacteristicInfo> {
        self.chars
            .iter()
            .map(|(&id, c)| {
                let p = c.properties;
                CharacteristicInfo {
                    id,
                    read: p.contains(CharPropFlags::READ),
                    write: p.contains(CharPropFlags::WRITE),
                    write_without_response: p.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE),
                    notify: p.contains(CharPropFlags::NOTIFY),
                    indicate: p.contains(CharPropFlags::INDICATE),
                }
            })
            .collect()
    }
    fn read(&mut self, id: u16) -> Result<Vec<u8>> {
        let c = self.characteristic(id)?;
        let result = self.rt.block_on(bounded(self.peripheral.read(&c)));
        self.finish(result)
    }
    fn write(&mut self, id: u16, bytes: &[u8], without_response: bool) -> Result<()> {
        let c = self.characteristic(id)?;
        let kind = if without_response {
            WriteType::WithoutResponse
        } else {
            WriteType::WithResponse
        };
        let result = self
            .rt
            .block_on(bounded(self.peripheral.write(&c, bytes, kind)));
        self.finish(result)
    }
    fn subscribe(&mut self, id: u16) -> Result<()> {
        let c = self.characteristic(id)?;
        let result = self.rt.block_on(bounded(self.peripheral.subscribe(&c)));
        self.finish(result)
    }
    fn receive(&mut self, duration: Duration) -> Result<Option<Notification>> {
        ensure!(
            !self.closed,
            "Bluetooth disconnected or receive queue failed; reconnect"
        );
        // Check the link before draining the queue: the gateway's PC-link
        // stop receipt can be queued just before the Bluetooth shutdown it
        // schedules, and is still a valid reply.
        let live = self.ready().and_then(|()| {
            ensure!(
                self.rt.block_on(bounded(self.peripheral.is_connected()))?,
                "Bluetooth peripheral disconnected"
            );
            Ok(())
        });
        let overflow = self.overflow.load(Ordering::Acquire);
        if let Some(n) = take_queued(&mut self.rx, overflow, live)? {
            return Ok(Some(n));
        }
        if duration.is_zero() {
            return Ok(None);
        }
        let result = self.rt.block_on(async {
            match timeout(duration, self.rx.recv()).await {
                Ok(Some(n)) => Ok(Some(n)),
                Ok(None) => bail!("Bluetooth notification stream closed"),
                Err(_) => {
                    ensure!(
                        bounded(self.peripheral.is_connected()).await?,
                        "Bluetooth peripheral disconnected"
                    );
                    Ok(None)
                }
            }
        });
        self.finish(result)
    }
    fn passkey(&mut self) -> Result<String> {
        if let Some(key) = self.credential.take() {
            return Ok(key);
        }
        Ok(rpassword::prompt_password(
            "E-TUBE six-digit BLE passkey: ",
        )?)
    }
    fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        if let Some(mut credential) = self.credential.take() {
            credential.zeroize();
        }
        self.pump.abort();
        self.rt.block_on(async {
            match bounded(self.peripheral.disconnect()).await {
                Ok(()) => Ok(()),
                Err(e) => already_closed(e, bounded(self.peripheral.is_connected()).await),
            }
        })
    }
}
/// A peripheral that dropped the link itself (the gateway does after its
/// PC-link stop) is closed, though BlueZ refuses its Disconnect with
/// NotConnected.
fn already_closed(error: anyhow::Error, connected: Result<bool>) -> Result<()> {
    match connected {
        Ok(false) => Ok(()),
        _ => Err(error),
    }
}
/// Return a notification queued before `live` was checked, even when that
/// check failed. A dropped notification leaves the queue incomplete, so an
/// overflowed queue is never drained.
fn take_queued(
    rx: &mut mpsc::Receiver<Notification>,
    overflow: bool,
    live: Result<()>,
) -> Result<Option<Notification>> {
    ensure!(!overflow, "Bluetooth receive queue overflowed; reconnect");
    match rx.try_recv() {
        Ok(n) => Ok(Some(n)),
        Err(mpsc::error::TryRecvError::Empty) => live.map(|()| None),
        Err(mpsc::error::TryRecvError::Disconnected) => {
            live?;
            bail!("Bluetooth notification stream closed")
        }
    }
}
impl Drop for NativeGatt {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn note(value: u8) -> Notification {
        Notification {
            characteristic: FREE_RESPONSE,
            value: vec![value],
        }
    }
    #[test]
    fn queued_replies_outlive_the_link_but_not_a_queue_overflow() {
        let down = || Err(anyhow::anyhow!("Bluetooth peripheral disconnected"));
        let (tx, mut rx) = mpsc::channel(4);
        tx.try_send(note(1)).unwrap();
        tx.try_send(note(2)).unwrap();
        assert!(take_queued(&mut rx, true, Ok(())).is_err());
        drop(tx);
        let first = take_queued(&mut rx, false, down()).unwrap().unwrap();
        assert_eq!(first.value, [1]);
        let second = take_queued(&mut rx, false, Ok(())).unwrap().unwrap();
        assert_eq!(second.value, [2]);
        let drained = take_queued(&mut rx, false, down()).unwrap_err();
        assert!(drained.to_string().contains("disconnected"));
        let ended = take_queued(&mut rx, false, Ok(())).unwrap_err();
        assert!(ended.to_string().contains("stream closed"));
        let (_tx, mut rx) = mpsc::channel(1);
        assert!(take_queued(&mut rx, false, Ok(())).unwrap().is_none());
        assert!(take_queued(&mut rx, false, down()).is_err());
    }
    #[test]
    fn closing_a_link_the_peripheral_dropped_succeeds() {
        let refused = || anyhow::anyhow!("org.bluez.Error.NotConnected");
        already_closed(refused(), Ok(false)).unwrap();
        // Still connected, or unknown: the failed disconnect stands.
        assert!(already_closed(refused(), Ok(true)).is_err());
        assert!(already_closed(refused(), Err(anyhow::anyhow!("timed out"))).is_err());
    }
}
