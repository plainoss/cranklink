//! Native GATT boundary. The Tokio runtime and bounded notification pump belong
//! to one connection; no async work survives close/drop.
use super::*;
use btleplug::{
    api::{
        Central, CentralEvent, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType,
    },
    platform::{Adapter, Manager, Peripheral},
};
use futures_util::{
    future::{select, Either},
    pin_mut, StreamExt,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
    time::timeout,
};
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
/// A latched adapter event, independent of btleplug's cached peripheral map.
/// Its macOS backend can leave state/disconnect futures pending after removing
/// a disconnected peripheral. Wake in-flight calls instead of waiting for them.
struct LinkState(watch::Sender<bool>);
impl LinkState {
    fn new() -> Self {
        Self(watch::channel(false).0)
    }
    fn disconnected(&self) -> bool {
        *self.0.borrow()
    }
    fn observe<T: PartialEq>(&self, disconnected: Option<&T>, target: &T) {
        if disconnected == Some(target) {
            self.0.send_replace(true);
        }
    }
    async fn bounded<T>(
        &self,
        future: impl Future<Output = std::result::Result<T, btleplug::Error>>,
    ) -> Result<T> {
        let mut state = self.0.subscribe();
        ensure!(!*state.borrow(), "Bluetooth peripheral disconnected");
        let disconnected = state.wait_for(|down| *down);
        let operation = bounded(future);
        pin_mut!(disconnected, operation);
        let result = match select(disconnected, operation).await {
            Either::Left(_) => bail!("Bluetooth peripheral disconnected"),
            Either::Right((result, _)) => result,
        };
        ensure!(!self.disconnected(), "Bluetooth peripheral disconnected");
        result
    }
}
// Poll only for discovery, never for connection or protocol retries. Return as
// soon as the requested device appears so short advertising windows remain usable.
async fn poll_discovery<T, F, Fut>(duration: Duration, mut poll: F) -> Result<Vec<T>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<T>>>,
{
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        let found = poll().await?;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if !found.is_empty() || remaining.is_zero() {
            return Ok(found);
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(100))).await;
    }
}
async fn discover(
    seconds: u64,
    target: Option<&str>,
) -> Result<Vec<(PeripheralInfo, Peripheral, Adapter)>> {
    let manager = bounded(Manager::new()).await?;
    let adapters = bounded(manager.adapters()).await?;
    ensure!(!adapters.is_empty(), "no Bluetooth adapters available");
    let mut started = Vec::new();
    let result: Result<Vec<(PeripheralInfo, Peripheral, Adapter)>> = async {
        for adapter in &adapters {
            bounded(adapter.start_scan(ScanFilter::default())).await?;
            started.push(adapter);
        }
        let snapshot = || async {
            let mut found = Vec::new();
            for adapter in &adapters {
                for p in bounded(adapter.peripherals()).await? {
                    let id = p.id().to_string();
                    if target.is_some_and(|target| target != id) {
                        continue;
                    }
                    if let Some(props) = bounded(p.properties()).await? {
                        found.push((
                            PeripheralInfo {
                                id,
                                name: props.local_name,
                                rssi: props.rssi,
                                etube_service_advertised: props
                                    .services
                                    .contains(&Uuid::parse_str(SERVICE)?),
                            },
                            p,
                            adapter.clone(),
                        ));
                    }
                }
            }
            Ok(found)
        };
        let duration = Duration::from_secs(seconds);
        if target.is_some() {
            poll_discovery(duration, snapshot).await
        } else {
            // User-requested scans retain their full observation window.
            tokio::time::sleep(duration).await;
            snapshot().await
        }
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
        .block_on(discover(seconds, None))?
        .into_iter()
        .map(|(info, _, _)| info)
        .filter(|i| all || i.etube_service_advertised)
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}
pub(super) struct NativeGatt {
    rt: Runtime,
    peripheral: Peripheral,
    _adapter: Adapter,
    chars: BTreeMap<u16, btleplug::api::Characteristic>,
    rx: mpsc::Receiver<Notification>,
    pump: JoinHandle<()>,
    event_pump: JoinHandle<()>,
    link: Arc<LinkState>,
    failed: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
    credential: Option<String>,
    closed: bool,
}
impl NativeGatt {
    pub fn open(id: &str, credential: Option<String>) -> Result<Self> {
        let rt = runtime()?;
        let mut matches: Vec<_> = rt
            .block_on(discover(5, Some(id)))?
            .into_iter()
            .filter(|(i, _, _)| i.id == id)
            .collect();
        ensure!(
            matches.len() == 1,
            "expected one BLE peripheral with ID {id}; found {} (run ble scan)",
            matches.len()
        );
        let (_, peripheral, adapter) = matches.remove(0);
        let link = Arc::new(LinkState::new());
        let failed = Arc::new(AtomicBool::new(false));
        // Subscribe on the discovery adapter before connecting so a fast drop
        // cannot fall between connection setup and event registration.
        let event_pump = rt.block_on(async {
            let mut events = bounded(adapter.events()).await?;
            let target = peripheral.id();
            let link = link.clone();
            let failed = failed.clone();
            Ok::<_, anyhow::Error>(tokio::spawn(async move {
                while let Some(event) = events.next().await {
                    let disconnected = match &event {
                        CentralEvent::DeviceDisconnected(id) => Some(id),
                        _ => None,
                    };
                    link.observe(disconnected, &target);
                    if link.disconnected() {
                        failed.store(true, Ordering::Release);
                        return;
                    }
                }
                failed.store(true, Ordering::Release);
            }))
        })?;
        let setup = rt.block_on(async {
            link.bounded(peripheral.connect())
                .await
                .context("connect native BLE peripheral")?;
            link.bounded(peripheral.discover_services())
                .await
                .context("discover native BLE services")?;
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
            let mut notifications = link.bounded(peripheral.notifications()).await?;
            let (tx, rx) = mpsc::channel(256);
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
            Ok((chars, rx, pump, overflow))
        });
        let (chars, rx, pump, overflow) = match setup {
            Ok(x) => x,
            Err(e) => {
                if !link.disconnected() {
                    let _ = rt.block_on(link.bounded(peripheral.disconnect()));
                }
                event_pump.abort();
                return Err(e);
            }
        };
        Ok(Self {
            rt,
            peripheral,
            _adapter: adapter,
            chars,
            rx,
            pump,
            event_pump,
            link,
            failed,
            overflow,
            credential,
            closed: false,
        })
    }
    fn ready(&self) -> Result<()> {
        ensure!(
            !self.closed && !self.link.disconnected() && !self.failed.load(Ordering::Acquire),
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
        let result = self
            .rt
            .block_on(self.link.bounded(self.peripheral.read(&c)));
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
            .block_on(self.link.bounded(self.peripheral.write(&c, bytes, kind)));
        self.finish(result)
    }
    fn subscribe(&mut self, id: u16) -> Result<()> {
        let c = self.characteristic(id)?;
        let result = self
            .rt
            .block_on(self.link.bounded(self.peripheral.subscribe(&c)));
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
                self.rt
                    .block_on(self.link.bounded(self.peripheral.is_connected()))?,
                "Bluetooth peripheral disconnected"
            );
            Ok(())
        });
        if let Some(n) = take_queued(&mut self.rx, &self.overflow, live)? {
            return Ok(Some(n));
        }
        if duration.is_zero() {
            return Ok(None);
        }
        let result = self.rt.block_on(reject_overflow(
            async {
                let mut state = self.link.0.subscribe();
                let received = {
                    let disconnected = state.wait_for(|down| *down);
                    let receiving = timeout(duration, self.rx.recv());
                    pin_mut!(disconnected, receiving);
                    match select(disconnected, receiving).await {
                        Either::Left(_) => None,
                        Either::Right((received, _)) => Some(received),
                    }
                };
                let Some(received) = received else {
                    // Preserve any final receipt already queued before the
                    // radio dropped, as in the synchronous queue path.
                    return take_queued(
                        &mut self.rx,
                        &self.overflow,
                        Err(anyhow::anyhow!("Bluetooth peripheral disconnected")),
                    );
                };
                match received {
                    Ok(Some(n)) => Ok(Some(n)),
                    Ok(None) => bail!("Bluetooth notification stream closed"),
                    Err(_) => {
                        ensure!(
                            self.link.bounded(self.peripheral.is_connected()).await?,
                            "Bluetooth peripheral disconnected"
                        );
                        Ok(None)
                    }
                }
            },
            &self.overflow,
        ));
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
        let result = self.rt.block_on(async {
            if self.link.disconnected() {
                return Ok(());
            }
            match self.link.bounded(self.peripheral.disconnect()).await {
                Ok(()) => Ok(()),
                Err(_) if self.link.disconnected() => Ok(()),
                Err(e) => {
                    let connected = self.link.bounded(self.peripheral.is_connected()).await;
                    if self.link.disconnected() {
                        Ok(())
                    } else {
                        already_closed(e, connected)
                    }
                }
            }
        });
        self.event_pump.abort();
        result
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
    overflow: &AtomicBool,
    live: Result<()>,
) -> Result<Option<Notification>> {
    ensure!(
        !overflow.load(Ordering::Acquire),
        "Bluetooth receive queue overflowed; reconnect"
    );
    let result = match rx.try_recv() {
        Ok(n) => Ok(Some(n)),
        Err(mpsc::error::TryRecvError::Empty) => live.map(|()| None),
        Err(mpsc::error::TryRecvError::Disconnected) => {
            live?;
            bail!("Bluetooth notification stream closed")
        }
    };
    ensure!(
        !overflow.load(Ordering::Acquire),
        "Bluetooth receive queue overflowed; reconnect"
    );
    result
}
/// The pump can fail while a receiver is suspended. Never accept a final
/// cleanup acknowledgement from a queue that became incomplete during await.
async fn reject_overflow<T>(
    future: impl Future<Output = Result<T>>,
    overflow: &AtomicBool,
) -> Result<T> {
    let result = future.await;
    ensure!(
        !overflow.load(Ordering::Acquire),
        "Bluetooth receive queue overflowed; reconnect"
    );
    result
}
impl Drop for NativeGatt {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnection_events_only_fence_the_selected_peripheral_and_stay_latched() {
        let link = LinkState::new();
        link.observe(Some(&2), &1);
        link.observe(None, &1);
        assert!(!link.disconnected());
        runtime()
            .unwrap()
            .block_on(link.bounded(async { Ok::<_, btleplug::Error>(()) }))
            .unwrap();
        link.observe(Some(&1), &1);
        link.observe(Some(&2), &1);
        assert!(link.disconnected());
        let touched = AtomicBool::new(false);
        let error = runtime()
            .unwrap()
            .block_on(link.bounded(async {
                touched.store(true, Ordering::Release);
                Ok::<_, btleplug::Error>(())
            }))
            .unwrap_err();
        assert!(error.to_string().contains("disconnected"));
        assert!(!touched.load(Ordering::Acquire));
    }
    #[test]
    fn disconnect_event_wakes_an_operation_that_would_never_reply() {
        let link = Arc::new(LinkState::new());
        let disconnect = link.clone();
        let error = runtime().unwrap().block_on(async {
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                disconnect.observe(Some(&1), &1);
            });
            timeout(
                Duration::from_secs(1),
                link.bounded(std::future::pending::<
                    std::result::Result<(), btleplug::Error>,
                >()),
            )
            .await
            .expect("disconnect event must wake pending state/disconnect calls")
            .unwrap_err()
        });
        assert!(error.to_string().contains("disconnected"));
    }
    #[test]
    fn disconnect_racing_an_operation_result_still_fences_the_result() {
        let link = LinkState::new();
        let error = runtime()
            .unwrap()
            .block_on(link.bounded(async {
                link.observe(Some(&1), &1);
                Ok::<_, btleplug::Error>(())
            }))
            .unwrap_err();
        assert!(error.to_string().contains("disconnected"));
    }
    #[test]
    fn discovery_returns_without_waiting_when_target_is_already_visible() {
        let rt = runtime().unwrap();
        // A zero-duration outer timeout still polls its future once. This fails
        // if discovery inserts any sleep before returning an available device.
        let found = rt.block_on(async {
            timeout(
                Duration::ZERO,
                poll_discovery(Duration::from_secs(5), || async { Ok(vec![42]) }),
            )
            .await
            .unwrap()
            .unwrap()
        });
        assert_eq!(found, [42]);
    }
    #[test]
    fn discovery_waits_for_a_later_advertisement_and_stops_after_finding_it() {
        let mut calls = 0;
        let found = runtime()
            .unwrap()
            .block_on(poll_discovery(Duration::from_secs(5), || {
                calls += 1;
                std::future::ready(Ok(if calls == 2 { vec![42] } else { vec![] }))
            }))
            .unwrap();
        assert_eq!(found, [42]);
        assert_eq!(calls, 2);
    }
    #[test]
    fn discovery_ends_when_absent_and_does_not_retry_backend_errors() {
        let rt = runtime().unwrap();
        let found: Vec<u8> = rt
            .block_on(poll_discovery(Duration::ZERO, || async { Ok(vec![]) }))
            .unwrap();
        assert!(found.is_empty());
        let mut calls = 0;
        let error = rt
            .block_on(poll_discovery::<u8, _, _>(Duration::from_secs(5), || {
                calls += 1;
                std::future::ready(Err(anyhow::anyhow!("discovery failed")))
            }))
            .unwrap_err();
        assert!(error.to_string().contains("discovery failed"));
        assert_eq!(calls, 1);
    }
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
        let overflow = AtomicBool::new(false);
        tx.try_send(note(1)).unwrap();
        tx.try_send(note(2)).unwrap();
        overflow.store(true, Ordering::Release);
        assert!(take_queued(&mut rx, &overflow, Ok(())).is_err());
        overflow.store(false, Ordering::Release);
        drop(tx);
        let first = take_queued(&mut rx, &overflow, down()).unwrap().unwrap();
        assert_eq!(first.value, [1]);
        let second = take_queued(&mut rx, &overflow, Ok(())).unwrap().unwrap();
        assert_eq!(second.value, [2]);
        let drained = take_queued(&mut rx, &overflow, down()).unwrap_err();
        assert!(drained.to_string().contains("disconnected"));
        let ended = take_queued(&mut rx, &overflow, Ok(())).unwrap_err();
        assert!(ended.to_string().contains("stream closed"));
        let (_tx, mut rx) = mpsc::channel(1);
        assert!(take_queued(&mut rx, &overflow, Ok(())).unwrap().is_none());
        assert!(take_queued(&mut rx, &overflow, down()).is_err());
    }
    #[test]
    fn overflow_during_await_rejects_an_otherwise_valid_final_receipt() {
        let rt = runtime().unwrap();
        let overflow = Arc::new(AtomicBool::new(false));
        let sender_overflow = overflow.clone();
        let (tx, mut rx) = mpsc::channel(1);
        let result = rt.block_on(async {
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                sender_overflow.store(true, Ordering::Release);
                tx.send(note(0)).await.unwrap();
            });
            reject_overflow(
                async { rx.recv().await.context("stream closed") },
                &overflow,
            )
            .await
        });
        assert!(result.unwrap_err().to_string().contains("queue overflowed"));
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
