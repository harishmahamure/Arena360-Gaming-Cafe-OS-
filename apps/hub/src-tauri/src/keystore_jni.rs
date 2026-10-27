//! The Android Keystore, reached from Rust through JNI.
//!
//! The AES-GCM key lives in the Android Keystore and is used by the Kotlin `KeystoreBridge`
//! (`wrap` / `unwrap`); it never crosses into Rust. This module only moves opaque byte arrays,
//! and turns the bridge's answers into `arena_tasks::SecretWrapper` results: a `null` from
//! `unwrap` is a lost key, an exception is a failure that may pass.

use arena_tasks::{SecretWrapper, WrapError};
use jni::objects::{GlobalRef, JByteArray, JClass, JValue};
use jni::{JNIEnv, JavaVM};

const BRIDGE_CLASS: &str = "in/arena360/hub/KeystoreBridge";

pub struct JniWrapper {
    vm: JavaVM,
    bridge: GlobalRef,
}

impl JniWrapper {
    /// Caches the VM and the bridge class. Call it from a Java thread (the app's class loader
    /// is only reachable there), such as the one that calls `nativeStart`.
    pub fn new(env: &mut JNIEnv) -> Result<Self, String> {
        let vm = env.get_java_vm().map_err(|e| format!("java vm: {e}"))?;
        let class = env
            .find_class(BRIDGE_CLASS)
            .map_err(|e| format!("{BRIDGE_CLASS}: {e}"))?;
        let bridge = env
            .new_global_ref(class)
            .map_err(|e| format!("global ref: {e}"))?;
        Ok(Self { vm, bridge })
    }

    fn call(&self, method: &str, input: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|e| format!("attach: {e}"))?;
        let result = self.invoke(&mut env, method, input);
        if env.exception_check().unwrap_or(false) {
            // The bridge threw: never leave it pending on this thread.
            let _ = env.exception_clear();
        }
        result
    }

    fn invoke(
        &self,
        env: &mut JNIEnv,
        method: &str,
        input: &[u8],
    ) -> Result<Option<Vec<u8>>, String> {
        let arg = env
            .byte_array_from_slice(input)
            .map_err(|e| format!("byte array: {e}"))?;
        // SAFETY: the global ref holds the `KeystoreBridge` class object found in `new`.
        let class = unsafe { JClass::from_raw(self.bridge.as_obj().as_raw()) };
        let out = env
            .call_static_method(&class, method, "([B)[B", &[JValue::Object(&arg)])
            .map_err(|e| format!("{method}: {e}"))?
            .l()
            .map_err(|e| format!("{method}: {e}"))?;
        if out.is_null() {
            return Ok(None);
        }
        let out = JByteArray::from(out);
        env.convert_byte_array(&out)
            .map(Some)
            .map_err(|e| format!("{method}: {e}"))
    }
}

impl SecretWrapper for JniWrapper {
    fn wrap(&self, plain: &[u8]) -> Result<Vec<u8>, WrapError> {
        match self.call("wrap", plain) {
            Ok(Some(wrapped)) => Ok(wrapped),
            Ok(None) => Err(WrapError::Failed("the Keystore returned nothing".into())),
            Err(e) => Err(WrapError::Failed(e)),
        }
    }

    fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>, WrapError> {
        match self.call("unwrap", wrapped) {
            Ok(Some(plain)) => Ok(plain),
            Ok(None) => Err(WrapError::Lost),
            Err(e) => Err(WrapError::Failed(e)),
        }
    }
}
