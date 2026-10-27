//! The backup folder the owner picked, reached from Rust through JNI.
//!
//! The Kotlin `BackupFolderBridge` holds the grant for a document tree URI and writes and deletes
//! a probe file in it. This module is the `arena_hub::BackupFolder` the Hub checks the
//! location with: it moves one string each way.

use std::sync::Arc;

use arena_hub::BackupFolder;
use arena_proto::BackupDirProblem;
use jni::objects::{GlobalRef, JClass, JString, JValue};
use jni::{JNIEnv, JavaVM};

const BRIDGE_CLASS: &str = "in/arena360/hub/BackupFolderBridge";

pub struct BackupFolderJni {
    vm: JavaVM,
    class: GlobalRef,
}

/// Finds the bridge class. Call it from a Java thread (the app's class loader is only reachable
/// there), such as the one that calls `nativeStart`.
pub fn connect(env: &mut JNIEnv) -> Result<Arc<dyn BackupFolder>, String> {
    let class = env
        .find_class(BRIDGE_CLASS)
        .map_err(|e| format!("{BRIDGE_CLASS}: {e}"))?;
    let class = env
        .new_global_ref(class)
        .map_err(|e| format!("global ref: {e}"))?;
    let vm = env.get_java_vm().map_err(|e| format!("java vm: {e}"))?;
    Ok(Arc::new(BackupFolderJni { vm, class }))
}

/// The problem a bridge reason word stands for; anything unknown counts as not found.
fn problem_of(reason: &str) -> BackupDirProblem {
    match reason {
        "not_dir" => BackupDirProblem::NotDir,
        "not_writable" => BackupDirProblem::NotWritable,
        _ => BackupDirProblem::NotFound,
    }
}

impl BackupFolderJni {
    fn ask(&self, location: &str) -> Result<Option<String>, String> {
        let mut env = self
            .vm
            .attach_current_thread()
            .map_err(|e| format!("attach: {e}"))?;
        // SAFETY: the global ref holds the `BackupFolderBridge` class object found in `connect`.
        let class = unsafe { JClass::from_raw(self.class.as_obj().as_raw()) };
        // A native thread never returns to Java, so a frame frees its local references.
        let mut result = None;
        let _ = env.with_local_frame(8, |env| -> Result<(), jni::errors::Error> {
            result = Some(call_probe(env, &class, location));
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

fn call_probe(env: &mut JNIEnv, class: &JClass, location: &str) -> Result<Option<String>, String> {
    let at = env
        .new_string(location)
        .map_err(|e| format!("string: {e}"))?;
    let out = env
        .call_static_method(
            class,
            "probe",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&at)],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("probe: {e}"))?;
    if out.is_null() {
        return Ok(None);
    }
    let text: String = env
        .get_string(&JString::from(out))
        .map_err(|e| format!("probe: {e}"))?
        .into();
    Ok(Some(text))
}

impl BackupFolder for BackupFolderJni {
    fn probe(&self, location: &str) -> Result<(), BackupDirProblem> {
        match self.ask(location) {
            Ok(None) => Ok(()),
            Ok(Some(reason)) => Err(problem_of(&reason)),
            Err(e) => {
                eprintln!("arena-hub: the backup folder bridge failed: {e}");
                Err(BackupDirProblem::NotFound)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_reasons_map_to_the_hub_problems() {
        assert_eq!(problem_of("not_dir"), BackupDirProblem::NotDir);
        assert_eq!(problem_of("not_writable"), BackupDirProblem::NotWritable);
        assert_eq!(problem_of("not_found"), BackupDirProblem::NotFound);
        assert_eq!(problem_of("anything else"), BackupDirProblem::NotFound);
    }
}
