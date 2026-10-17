//! Offline GATT boundary. The socket peer models the radio; it executes profiled
//! gateway/motor protocol firmware. This does not simulate Bluetooth RF or pairing.
use super::{
    CharacteristicInfo, Gatt, Notification, AUTH_CONTROL, AUTH_NONCE, FEATURE, FREE_COMMAND,
    FREE_RESPONSE, PCE_COMMAND, PCE_RESPONSE,
};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};
use zeroize::Zeroize;

const MAX_LINE: usize = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    error: Option<String>,
    bytes: Option<Vec<u8>>,
    notification: Option<Notification>,
}

pub struct SimulatorGatt {
    socket: BufReader<UnixStream>,
    closed: bool,
    credential: Option<String>,
}
impl SimulatorGatt {
    pub fn open(path: &str) -> Result<Self> {
        ensure!(
            Path::new(path).is_absolute(),
            "BLE simulator requires an absolute Unix socket path"
        );
        let socket = UnixStream::connect(path).context("connect offline BLE simulator")?;
        socket.set_read_timeout(Some(IO_TIMEOUT))?;
        socket.set_write_timeout(Some(IO_TIMEOUT))?;
        Ok(Self {
            socket: BufReader::new(socket),
            closed: false,
            credential: None,
        })
    }
    pub fn with_passkey(mut self, credential: Option<String>) -> Self {
        self.credential = credential;
        self
    }
    fn terminate(&mut self) {
        if let Some(mut credential) = self.credential.take() {
            credential.zeroize();
        }
        self.closed = true;
        let _ = self.socket.get_mut().shutdown(Shutdown::Both);
    }
    fn rpc(&mut self, request: Value, timeout: Duration) -> Result<Reply> {
        ensure!(!self.closed, "BLE simulator connection is closed");
        let result = self.exchange(request, timeout);
        if result.is_err() {
            self.terminate();
        }
        result
    }
    fn exchange(&mut self, request: Value, timeout: Duration) -> Result<Reply> {
        self.socket.get_mut().set_read_timeout(Some(timeout))?;
        let mut bytes = serde_json::to_vec(&request)?;
        ensure!(bytes.len() < MAX_LINE, "oversized BLE simulator request");
        bytes.push(b'\n');
        self.socket
            .get_mut()
            .write_all(&bytes)
            .context("write BLE simulator request")?;
        self.socket.get_mut().flush()?;
        // Read at most one bounded line, preserving partial-read and EOF detection.
        let mut response = Vec::new();
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            ensure!(
                !remaining.is_zero(),
                "BLE simulator response deadline exceeded"
            );
            self.socket.get_mut().set_read_timeout(Some(remaining))?;
            let available = self
                .socket
                .fill_buf()
                .context("read BLE simulator response")?;
            ensure!(
                !available.is_empty(),
                "BLE simulator disconnected before response"
            );
            let count = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |i| i + 1);
            ensure!(
                response.len() + count <= MAX_LINE,
                "oversized BLE simulator response"
            );
            let complete = available[count - 1] == b'\n';
            response.extend_from_slice(&available[..count]);
            self.socket.consume(count);
            if complete {
                break;
            }
        }
        let reply: Reply =
            serde_json::from_slice(&response).context("invalid BLE simulator response")?;
        if !reply.ok {
            bail!(
                "BLE simulator rejected operation: {}",
                reply.error.as_deref().unwrap_or("unspecified error")
            );
        }
        Ok(reply)
    }
}
impl Gatt for SimulatorGatt {
    fn characteristics(&self) -> Vec<CharacteristicInfo> {
        [
            AUTH_CONTROL,
            AUTH_NONCE,
            PCE_RESPONSE,
            PCE_COMMAND,
            FREE_RESPONSE,
            FREE_COMMAND,
            FEATURE,
        ]
        .into_iter()
        .map(|id| CharacteristicInfo {
            id,
            read: matches!(id, AUTH_NONCE | FEATURE),
            write: matches!(id, AUTH_CONTROL | PCE_COMMAND | FREE_COMMAND | FEATURE),
            write_without_response: false,
            notify: matches!(id, AUTH_CONTROL | PCE_RESPONSE | FREE_RESPONSE),
            indicate: false,
        })
        .collect()
    }
    fn read(&mut self, id: u16) -> Result<Vec<u8>> {
        let result = (|| {
            let reply = self.rpc(json!({"op":"read", "id":id}), IO_TIMEOUT)?;
            let bytes = reply.bytes.context("BLE simulator read omitted bytes")?;
            ensure!(bytes.len() <= 20, "oversized BLE simulator characteristic");
            Ok(bytes)
        })();
        if result.is_err() {
            self.terminate();
        }
        result
    }
    fn write(&mut self, id: u16, bytes: &[u8], without_response: bool) -> Result<()> {
        if bytes.len() > 20 {
            self.terminate();
            bail!("oversized BLE simulator write");
        }
        self.rpc(
            json!({"op":"write", "id":id, "bytes":bytes,
            "without_response":without_response}),
            IO_TIMEOUT,
        )?;
        Ok(())
    }
    fn subscribe(&mut self, id: u16) -> Result<()> {
        self.rpc(json!({"op":"subscribe", "id":id}), IO_TIMEOUT)?;
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<Option<Notification>> {
        if timeout > Duration::from_secs(30) {
            self.terminate();
            bail!("BLE simulator receive timeout exceeds 30 seconds");
        }
        let reply = self.rpc(
            json!({"op":"receive", "timeout_ms":timeout.as_millis() as u64}),
            timeout.saturating_add(IO_TIMEOUT),
        )?;
        if let Some(n) = &reply.notification {
            if n.value.len() > 20
                || !matches!(
                    n.characteristic,
                    AUTH_CONTROL | PCE_RESPONSE | FREE_RESPONSE
                )
            {
                self.terminate();
                bail!("invalid BLE simulator notification");
            }
        }
        Ok(reply.notification)
    }
    fn passkey(&mut self) -> Result<String> {
        ensure!(!self.closed, "BLE simulator connection is closed");
        Ok(self
            .credential
            .take()
            .unwrap_or_else(|| "000000".to_owned()))
    }
    fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let result = self.rpc(json!({"op":"close"}), IO_TIMEOUT).map(|_| ());
        self.terminate();
        result
    }
}
impl Drop for SimulatorGatt {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
#[path = "simulator_tests.rs"]
mod tests;
