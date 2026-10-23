//! Android owns scanning, permission handling, connection/discovery and bounded
//! GATT operations. JNI adapts those operations to the shared synchronous core.
use anyhow::{ensure, Context, Result};
use cranklink::ble::{CharacteristicInfo, Gatt, Notification};
use jni::{
    objects::{GlobalRef, JByteArray, JObject, JString, JValue},
    JNIEnv, JavaVM,
};
use std::{sync::Arc, time::Duration};

pub(crate) struct JavaGatt {
    vm: Arc<JavaVM>,
    object: GlobalRef,
    characteristics: Vec<CharacteristicInfo>,
}

impl JavaGatt {
    pub(crate) fn new(env: &mut JNIEnv<'_>, object: GlobalRef) -> Result<Self> {
        let mut io = Self {
            vm: Arc::new(env.get_java_vm()?),
            object,
            characteristics: Vec::new(),
        };
        let encoded = io.call(|env, object| {
            let value = env
                .call_method(object, "characteristics", "()Ljava/lang/String;", &[])?
                .l()?;
            let text: String = env.get_string(&JString::from(value))?.into();
            Ok(text)
        })?;
        io.characteristics = serde_json::from_str(&encoded)
            .context("invalid Android GATT characteristic discovery")?;
        ensure!(
            io.characteristics.len() <= 64,
            "too many E-TUBE characteristics"
        );
        let mut unique = std::collections::BTreeSet::new();
        ensure!(
            io.characteristics.iter().all(|c| unique.insert(c.id)),
            "duplicate E-TUBE characteristics"
        );
        Ok(io)
    }

    fn call<T>(
        &self,
        call: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> jni::errors::Result<T>,
    ) -> Result<T> {
        let mut env = self
            .vm
            .attach_current_thread()
            .context("attach native GATT thread")?;
        // Full reads and ordered saves can make hundreds of JNI callbacks in
        // one invoke. Return only owned Rust data and release each callback's
        // Java arrays/strings/throwables before starting the next callback.
        env.with_local_frame(16, |env| match call(env, self.object.as_obj()) {
            Ok(result) => Ok(result),
            Err(error) => {
                let exception = env.exception_occurred().ok();
                let _ = env.exception_clear();
                let message = exception.filter(|e| !e.is_null()).and_then(|exception| {
                    let text = env
                        .call_method(exception, "toString", "()Ljava/lang/String;", &[])
                        .ok()?
                        .l()
                        .ok()?;
                    let string: String = env.get_string(&JString::from(text)).ok()?.into();
                    Some(string)
                });
                let _ = env.exception_clear();
                anyhow::bail!(
                    "Android GATT: {}",
                    message.unwrap_or_else(|| error.to_string())
                )
            }
        })
    }

    fn bytes(
        &self,
        method: &str,
        signature: &str,
        arguments: &[JValue<'_, '_>],
    ) -> Result<Vec<u8>> {
        self.call(|env, object| {
            let array = env.call_method(object, method, signature, arguments)?.l()?;
            env.convert_byte_array(JByteArray::from(array))
        })
    }
}

impl Gatt for JavaGatt {
    fn characteristics(&self) -> Vec<CharacteristicInfo> {
        self.characteristics.clone()
    }

    fn read(&mut self, id: u16) -> Result<Vec<u8>> {
        self.bytes("read", "(I)[B", &[JValue::Int(i32::from(id))])
    }

    fn write(&mut self, id: u16, bytes: &[u8], without_response: bool) -> Result<()> {
        self.call(|env, object| {
            let array = env.byte_array_from_slice(bytes)?;
            env.call_method(
                object,
                "write",
                "(I[BZ)V",
                &[
                    JValue::Int(i32::from(id)),
                    JValue::Object(array.as_ref()),
                    JValue::Bool(u8::from(without_response)),
                ],
            )?;
            Ok(())
        })
    }

    fn subscribe(&mut self, id: u16) -> Result<()> {
        self.call(|env, object| {
            env.call_method(object, "subscribe", "(I)V", &[JValue::Int(i32::from(id))])?;
            Ok(())
        })
    }

    fn receive(&mut self, timeout: Duration) -> Result<Option<Notification>> {
        let milliseconds = timeout.as_millis().min(i64::MAX as u128) as i64;
        let bytes = self.bytes("receive", "(J)[B", &[JValue::Long(milliseconds)])?;
        if bytes.is_empty() {
            return Ok(None);
        }
        ensure!(
            bytes.len() >= 2 && bytes.len() <= 514,
            "invalid Android BLE notification length"
        );
        Ok(Some(Notification {
            characteristic: u16::from_le_bytes([bytes[0], bytes[1]]),
            value: bytes[2..].to_vec(),
        }))
    }

    fn passkey(&mut self) -> Result<String> {
        self.call(|env, object| {
            let text = env
                .call_method(object, "passkey", "()Ljava/lang/String;", &[])?
                .l()?;
            let result: String = env.get_string(&JString::from(text))?.into();
            Ok(result)
        })
    }

    fn clear_passkey(&mut self) {
        let _ = self.call(|env, object| {
            env.call_method(object, "clearPasskey", "()V", &[])?;
            Ok(())
        });
    }

    fn close(&mut self) -> Result<()> {
        self.call(|env, object| {
            env.call_method(object, "close", "()V", &[])?;
            Ok(())
        })
    }
}
