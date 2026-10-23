use super::*;
pub(super) struct BrowserGatt {
    chars: Vec<CharacteristicInfo>,
    credential: Option<String>,
    closed: bool,
}
impl BrowserGatt {
    pub fn open(credential: Option<String>) -> Result<Self> {
        // Covers OPEN_WORST_CASE_MS in ble.js: 3 x 12 s connect attempts,
        // 0.8 s + 1.6 s backoff and 2 x 10 s discovery (58.4 s). host.js
        // adds its 5 s margin on top.
        let bytes = crate::wasm::rpc("ble.open", &[], 60000)?;
        Ok(Self {
            chars: serde_json::from_slice(&bytes)?,
            credential,
            closed: false,
        })
    }
    fn request(&self, op: &str, data: serde_json::Value, timeout: u32) -> Result<Vec<u8>> {
        ensure!(!self.closed, "Bluetooth connection is closed");
        Ok(crate::wasm::rpc(op, &serde_json::to_vec(&data)?, timeout)?)
    }
}
impl Gatt for BrowserGatt {
    fn characteristics(&self) -> Vec<CharacteristicInfo> {
        self.chars.clone()
    }
    fn read(&mut self, id: u16) -> Result<Vec<u8>> {
        self.request("ble.read", serde_json::json!({"id":id}), 10000)
    }
    fn write(&mut self, id: u16, bytes: &[u8], without_response: bool) -> Result<()> {
        self.request(
            "ble.write",
            serde_json::json!({"id":id,"bytes":bytes,"withoutResponse":without_response}),
            10000,
        )?;
        Ok(())
    }
    fn subscribe(&mut self, id: u16) -> Result<()> {
        self.request("ble.subscribe", serde_json::json!({"id":id}), 10000)?;
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<Option<Notification>> {
        // The JS queue polls in bounded 5 s slices. A poll timeout is not the
        // caller's firmware timeout (erase/reset/transfer can allow 6–25 s).
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            let milliseconds = if timeout.is_zero() {
                0
            } else {
                remaining.as_millis().clamp(1, 5000) as u32
            };
            let bytes = self.request("ble.receive", serde_json::json!({}), milliseconds)?;
            let notification: Option<Notification> = serde_json::from_slice(&bytes)?;
            if notification.is_some() || timeout.is_zero() || started.elapsed() >= timeout {
                return Ok(notification);
            }
        }
    }
    fn passkey(&mut self) -> Result<String> {
        if let Some(key) = self.credential.take() {
            return Ok(key);
        }
        // Prompt on the host may require human input; the host has its own
        // bounded dialog. The host may keep the passkey for this gateway in
        // tab session storage; it is never written to file storage.
        let bytes = self.request("ble.passkey", serde_json::json!({}), 120000)?;
        Ok(String::from_utf8(bytes)?)
    }
    fn clear_passkey(&mut self) {
        if let Some(mut credential) = self.credential.take() {
            credential.zeroize();
        }
        let _ = crate::wasm::rpc("ble.reset-passkey", &[], 10000);
    }
    fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        if let Some(mut credential) = self.credential.take() {
            credential.zeroize();
        }
        crate::wasm::rpc("ble.close", &[], 10000)?;
        Ok(())
    }
}
impl Drop for BrowserGatt {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
