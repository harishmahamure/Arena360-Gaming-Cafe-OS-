//! `NsdManager`, reached from Rust through JNI.
//!
//! The Kotlin `NsdBridge` registers the `_arena360-hub._tcp` service and re-registers it when
//! the network changes. This module turns the core's `Advertiser` calls into bridge calls, names
//! the network for Settings → Devices, and carries the bridge's "the network changed" call back
//! into the core as a wake-up.

use std::sync::{Arc, OnceLock};

use arena_tasks::{Advert, Advertiser, NetworkName};
use jni::objects::{GlobalRef, JClass, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM};
use tokio::sync::Notify;

const BRIDGE_CLASS: &str = "in/arena360/hub/NsdBridge";

/// Woken by `NsdBridge.nativeNetworkChanged`; the Hub's mDNS task waits on it.
static NETWORK_WAKE: OnceLock<Arc<Notify>> = OnceLock::new();

pub fn network_wake() -> Arc<Notify> {
    Arc::clone(NETWORK_WAKE.get_or_init(|| Arc::new(Notify::new())))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_in_arena360_hub_NsdBridge_nativeNetworkChanged(
    _env: JNIEnv,
    _class: JClass,
) {
    network_wake().notify_one();
}

/// The cached VM and the bridge class, shared by the advertiser and the network namer.
struct Bridge {
    vm: JavaVM,
    class: GlobalRef,
}

impl Bridge {
    fn with_env<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv, &JClass) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|e| format!("attach: {e}"))?;
        // SAFETY: the global ref holds the `NsdBridge` class object found in `connect`.
        let class = unsafe { JClass::from_raw(self.class.as_obj().as_raw()) };
        let result = f(&mut env, &class);
        if env.exception_check().unwrap_or(false) {
            // The bridge threw: never leave it pending on this thread.
            let _ = env.exception_clear();
        }
        result
    }
}

/// Finds the bridge class. Call it from a Java thread (the app's class loader is only reachable
/// there), such as the one that calls `nativeStart`.
pub fn connect(env: &mut JNIEnv) -> Result<(NsdAdvertiser, NsdNetwork), String> {
    let class = env
        .find_class(BRIDGE_CLASS)
        .map_err(|e| format!("{BRIDGE_CLASS}: {e}"))?;
    let class = env
        .new_global_ref(class)
        .map_err(|e| format!("global ref: {e}"))?;
    let vm = env.get_java_vm().map_err(|e| format!("java vm: {e}"))?;
    let second = env.get_java_vm().map_err(|e| format!("java vm: {e}"))?;
    let bridge = Arc::new(Bridge { vm, class });
    let namer = Arc::new(Bridge {
        vm: second,
        class: bridge.class.clone(),
    });
    Ok((NsdAdvertiser { bridge }, NsdNetwork { bridge: namer }))
}

pub struct NsdAdvertiser {
    bridge: Arc<Bridge>,
}

fn strings<'a>(env: &mut JNIEnv<'a>, items: &[&str]) -> Result<JObjectArray<'a>, String> {
    let empty = JString::from(JObject::null());
    let array = env
        .new_object_array(items.len() as i32, "java/lang/String", &empty)
        .map_err(|e| format!("string array: {e}"))?;
    for (i, item) in items.iter().enumerate() {
        let s = env.new_string(item).map_err(|e| format!("string: {e}"))?;
        env.set_object_array_element(&array, i as i32, s)
            .map_err(|e| format!("string array: {e}"))?;
    }
    Ok(array)
}

impl Advertiser for NsdAdvertiser {
    fn register(&mut self, advert: &Advert) -> Result<(), String> {
        let keys: Vec<&str> = advert.txt.iter().map(|(k, _)| k.as_str()).collect();
        let values: Vec<&str> = advert.txt.iter().map(|(_, v)| v.as_str()).collect();
        let accepted = self.bridge.with_env(|env, class| {
            let instance = env
                .new_string(&advert.instance)
                .map_err(|e| format!("string: {e}"))?;
            let keys = strings(env, &keys)?;
            let values = strings(env, &values)?;
            env.call_static_method(
                class,
                "register",
                "(Ljava/lang/String;I[Ljava/lang/String;[Ljava/lang/String;)Z",
                &[
                    JValue::Object(&instance),
                    JValue::Int(i32::from(advert.port)),
                    JValue::Object(&keys),
                    JValue::Object(&values),
                ],
            )
            .and_then(|v| v.z())
            .map_err(|e| format!("register: {e}"))
        })?;
        if accepted {
            Ok(())
        } else {
            Err("NsdManager is unavailable".into())
        }
    }

    fn unregister(&mut self) -> Result<(), String> {
        self.bridge.with_env(|env, class| {
            env.call_static_method(class, "unregister", "()V", &[])
                .map(|_| ())
                .map_err(|e| format!("unregister: {e}"))
        })
    }

    fn shutdown(&mut self) {
        let _ = self.unregister();
    }
}

pub struct NsdNetwork {
    bridge: Arc<Bridge>,
}

impl NetworkName for NsdNetwork {
    fn name(&self) -> Option<String> {
        self.bridge
            .with_env(|env, class| {
                let out = env
                    .call_static_method(class, "networkName", "()Ljava/lang/String;", &[])
                    .and_then(|v| v.l())
                    .map_err(|e| format!("networkName: {e}"))?;
                if out.is_null() {
                    return Ok(None);
                }
                let name: String = env
                    .get_string(&JString::from(out))
                    .map_err(|e| format!("networkName: {e}"))?
                    .into();
                Ok(Some(name))
            })
            .ok()
            .flatten()
    }
}
