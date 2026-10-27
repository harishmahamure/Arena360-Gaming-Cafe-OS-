//! Bluetooth SPP printing, reached from Rust through JNI.
//!
//! The Kotlin `BluetoothBridge` lists the printers paired in Android settings and sends a job
//! over a fresh RFCOMM connection. This module is the `arena_tasks::printer::BluetoothLink` the
//! Android printer adapter runs on: it moves strings and byte arrays, and turns the bridge's
//! answers into the adapter's failures.

use std::sync::Arc;
use std::time::Duration;

use arena_proto::BluetoothAccess;
use arena_tasks::printer::{BluetoothFailure, BluetoothLink, PairedPrinter, access_from_bridge};
use jni::objects::{GlobalRef, JClass, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM};

const BRIDGE_CLASS: &str = "in/arena360/hub/BluetoothBridge";

pub struct BluetoothJni {
    vm: JavaVM,
    class: GlobalRef,
}

/// Finds the bridge class. Call it from a Java thread (the app's class loader is only reachable
/// there), such as the one that calls `nativeStart`.
pub fn connect(env: &mut JNIEnv) -> Result<Arc<dyn BluetoothLink>, String> {
    let class = env
        .find_class(BRIDGE_CLASS)
        .map_err(|e| format!("{BRIDGE_CLASS}: {e}"))?;
    let class = env
        .new_global_ref(class)
        .map_err(|e| format!("global ref: {e}"))?;
    let vm = env.get_java_vm().map_err(|e| format!("java vm: {e}"))?;
    Ok(Arc::new(BluetoothJni { vm, class }))
}

impl BluetoothJni {
    fn with_env<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv, &JClass) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|e| format!("attach: {e}"))?;
        // SAFETY: the global ref holds the `BluetoothBridge` class object found in `connect`.
        let class = unsafe { JClass::from_raw(self.class.as_obj().as_raw()) };
        // A native thread never returns to Java, so a frame frees its local references.
        let mut result = None;
        let _ = env.with_local_frame(32, |env| -> Result<(), jni::errors::Error> {
            result = Some(f(env, &class));
            Ok(())
        });
        let result = result.unwrap_or_else(|| Err("no local frame".to_string()));
        if env.exception_check().unwrap_or(false) {
            // The bridge threw: never leave it pending on this thread.
            let _ = env.exception_clear();
        }
        result
    }
}

impl BluetoothLink for BluetoothJni {
    fn access(&self) -> BluetoothAccess {
        self.with_env(|env, class| {
            env.call_static_method(class, "access", "()I", &[])
                .and_then(|v| v.i())
                .map_err(|e| format!("access: {e}"))
        })
        .map_or(BluetoothAccess::Unavailable, access_from_bridge)
    }

    fn paired(&self) -> Result<Vec<PairedPrinter>, BluetoothFailure> {
        let listed = self
            .with_env(|env, class| {
                let out = env
                    .call_static_method(class, "paired", "()[Ljava/lang/String;", &[])
                    .and_then(|v| v.l())
                    .map_err(|e| format!("paired: {e}"))?;
                if out.is_null() {
                    return Ok(None);
                }
                let array = JObjectArray::from(out);
                let len = env
                    .get_array_length(&array)
                    .map_err(|e| format!("paired: {e}"))?;
                let mut items = Vec::new();
                for i in 0..len {
                    let item = env
                        .get_object_array_element(&array, i)
                        .map_err(|e| format!("paired: {e}"))?;
                    let text: String = env
                        .get_string(&JString::from(item))
                        .map_err(|e| format!("paired: {e}"))?
                        .into();
                    items.push(text);
                }
                Ok(Some(items))
            })
            .map_err(BluetoothFailure::Link)?;
        match listed {
            Some(items) => Ok(items
                .chunks_exact(2)
                .map(|pair| PairedPrinter {
                    address: pair[0].to_ascii_uppercase(),
                    name: pair[1].clone(),
                })
                .collect()),
            None => Err(match self.access() {
                BluetoothAccess::Denied => BluetoothFailure::Permission,
                _ => BluetoothFailure::Unavailable,
            }),
        }
    }

    fn send(&self, address: &str, bytes: &[u8], timeout: Duration) -> Result<(), BluetoothFailure> {
        let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        let answer = self
            .with_env(|env, class| {
                let to = env
                    .new_string(address)
                    .map_err(|e| format!("string: {e}"))?;
                let data = env
                    .byte_array_from_slice(bytes)
                    .map_err(|e| format!("byte array: {e}"))?;
                let out = env
                    .call_static_method(
                        class,
                        "send",
                        "(Ljava/lang/String;[BI)Ljava/lang/String;",
                        &[
                            JValue::Object(&to),
                            JValue::Object(&JObject::from(data)),
                            JValue::Int(timeout_ms),
                        ],
                    )
                    .and_then(|v| v.l())
                    .map_err(|e| format!("send: {e}"))?;
                if out.is_null() {
                    return Ok(None);
                }
                let text: String = env
                    .get_string(&JString::from(out))
                    .map_err(|e| format!("send: {e}"))?
                    .into();
                Ok(Some(text))
            })
            .map_err(BluetoothFailure::Link)?;
        match answer {
            None => Ok(()),
            Some(text) => Err(BluetoothFailure::from_bridge(&text)),
        }
    }
}
