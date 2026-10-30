//! `POST /ui/backup/export` (Story 9.4): one tap writes the whole database to the backup
//! location as `<branch_code>-<yyyy-mm-dd-hhmm>.a360` and records it with the `backup.export`
//! intent, so Settings → System can show it and the audit log names who did it.
//! `POST /ui/backup/export-everything` (Story 9.6) does the same into a folder of its own that
//! also holds every Hub report for a range as CSV and a `README.txt`.
//!
//! Order matters. The route refuses a follower Hub (`NOT_PRIMARY`) and a caller with neither the
//! `backup.export` permission nor a manager's PIN ticket (`PERMISSION_DENIED`) before any file
//! work. It then checks the location, writes the file (the writer is never blocked: the copy is
//! made on a connection of its own), and only then submits the intent, which redeems the ticket.
//! If the intent is refused the file (or folder) is deleted, so `backup.last_export` never names a
//! file the Hub did not stand behind. Two exceptions keep the file: a `HUB_SLOW` answer (the
//! intent may still commit) and a file that replaced an earlier export of the same minute.

use std::path::PathBuf;
use std::sync::Arc;

use arena_proto::{
    Actor, ActorKind, AppError, BackupExport, ErrorCode, Intent, IntentKind, IntentPayload,
    KEY_BACKUP_DIR, Permission, UiError,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::ui_auth::{
    UiAuth, auth_failed, bad_frame, bearer, fail, internal, player_kek, run_intent, ui_error,
};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ExportRequest {
    /// A manager's PIN ticket for `backup.export`, when the caller lacks the permission.
    #[serde(default)]
    elevation_ticket: Option<String>,
}

/// What a route made on disk: the facts `backup.export` records, the path to delete if the Hub
/// refuses to record them, and what to answer.
struct Made {
    facts: BackupExport,
    cleanup: PathBuf,
    reply: serde_json::Value,
    /// Whether `cleanup` is a file that may have replaced an earlier one of the same name.
    can_replace: bool,
}

/// Makes the files of one export from the shared reader, the backup location, the clock and the
/// key store (the Players report needs it). Blocking (disk).
type Make = Box<
    dyn FnOnce(
            Arc<arena_core::SnapshotReader>,
            String,
            i64,
            Option<Arc<dyn arena_core::players::Kek>>,
        ) -> Result<Made, AppError>
        + Send,
>;

/// `POST /ui/backup/export {elevation_ticket?}` answers the [`BackupExport`] it recorded.
pub(crate) async fn export(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    run(&auth, &headers, &body, |body| {
        let request = if body.iter().all(u8::is_ascii_whitespace) {
            ExportRequest::default()
        } else {
            serde_json::from_slice::<ExportRequest>(body)
                .map_err(|_| bad_frame("the body must be {\"elevation_ticket\"?}"))?
        };
        let make: Make = Box::new(|reader, folder, now, _| {
            let (facts, path) = reader.export_to_folder(&folder, now)?;
            Ok(Made {
                reply: serde_json::to_value(&facts).unwrap_or_default(),
                facts,
                cleanup: path,
                can_replace: true,
            })
        });
        Ok((request.elevation_ticket, make))
    })
    .await
}

const EVERYTHING_SHAPE: &str =
    "the body must be {\"from\": \"YYYY-MM-DD\", \"to\": \"YYYY-MM-DD\", \"elevation_ticket\"?}";

/// `POST /ui/backup/export-everything {from, to, elevation_ticket?}` (Story 9.6): the database
/// export plus every Hub report for the range as CSV and a `README.txt`, in a folder of its own
/// inside the backup location. Answers an [`arena_proto::EverythingExported`]. The database file
/// in it is recorded like any export; if the record is refused the whole folder is deleted.
pub(crate) async fn export_everything(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    run(&auth, &headers, &body, |body| {
        let request = serde_json::from_slice::<arena_proto::ExportEverythingRequest>(body)
            .map_err(|_| bad_frame(EVERYTHING_SHAPE))?;
        let make: Make = Box::new(move |reader, folder, now, kek| {
            let key = match &kek {
                Some(kek) => Some(kek.kek()?),
                None => None,
            };
            let (done, dir) = reader.export_everything(
                &folder,
                (request.from.as_str(), request.to.as_str()),
                now,
                key.as_ref(),
            )?;
            Ok(Made {
                facts: done.export.clone(),
                reply: serde_json::to_value(&done).unwrap_or_default(),
                cleanup: dir,
                can_replace: false,
            })
        });
        Ok((request.elevation_ticket, make))
    })
    .await
}

/// The steps both routes share, after `parse` has read the body into a ticket and the job.
async fn run(
    auth: &Arc<UiAuth>,
    headers: &HeaderMap,
    body: &[u8],
    parse: impl FnOnce(&[u8]) -> Result<(Option<String>, Make), UiError>,
) -> Response {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let (ticket, make) = match parse(body) {
        Ok(parsed) => parsed,
        Err(e) => return fail(e),
    };
    if auth.writer().is_none() {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    }
    if ticket.is_none() && !me.permissions.contains(&Permission::BackupExport) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "exporting the database needs a manager's PIN",
        )));
    }

    // One export at a time: two taps in the same minute would share a file name.
    let _single = auth.export_lock().lock().await;

    // A ticket that could not be redeemed would refuse the intent after the file exists. Checked
    // under the lock so a queued export cannot spend it between the check and the redemption.
    if let Some(ticket) = &ticket
        && let Err(e) = auth.check_ticket(ticket, &token, &me.staff_id, IntentKind::BackupExport)
    {
        return fail(e);
    }
    let reader = Arc::clone(auth.reader());
    let folder = {
        let reader = Arc::clone(&reader);
        match tokio::task::spawn_blocking(move || reader.setting_value(KEY_BACKUP_DIR)).await {
            Ok(Ok(value)) => value.unwrap_or_default(),
            Ok(Err(e)) => return fail(ui_error(e)),
            Err(_) => return fail(internal()),
        }
    };
    if let Err(e) = auth.probe_export_folder(&folder).await {
        return fail(e);
    }

    // File names have minute resolution, so a second export can replace an earlier one; a refusal
    // must not then delete that earlier file.
    let before: std::collections::HashSet<std::ffi::OsString> = std::fs::read_dir(&folder)
        .map(|dir| dir.flatten().map(|entry| entry.file_name()).collect())
        .unwrap_or_default();

    let now = auth.clock().now_ms();
    let kek = player_kek(auth).ok();
    let made = {
        let (reader, folder) = (Arc::clone(&reader), folder.clone());
        tokio::task::spawn_blocking(move || make(reader, folder, now, kek)).await
    };
    let Made {
        facts,
        cleanup,
        reply,
        can_replace,
    } = match made {
        Ok(Ok(done)) => done,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    tracing::info!(
        staff_id = %me.staff_id,
        file = %facts.file_name,
        bytes = facts.bytes,
        last_seq = facts.last_seq,
        "database exported"
    );

    let intent = Intent {
        intent_id: ulid::Ulid::generate().to_string(),
        kind: IntentKind::BackupExport,
        idempotency_key: ulid::Ulid::generate().to_string(),
        issued_at: now,
        base_rev: None,
        actor: Actor {
            kind: ActorKind::Staff,
            id: me.staff_id.clone(),
        },
        elevated_by: None,
        reason: None,
        payload: IntentPayload::BackupExport(facts.clone()),
    };
    let answer = run_intent(auth, &token, &me, intent, ticket).await;
    if answer.status().is_success() {
        return Json(reply).into_response();
    }
    let (parts, body) = answer.into_parts();
    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(_) => return fail(internal()),
    };
    let slow = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .is_some_and(|v| v["code"] == serde_json::json!(ErrorCode::HubSlow));
    let replaced = can_replace && before.contains(std::ffi::OsStr::new(&facts.file_name));
    if !slow && !replaced {
        let removed = if cleanup.is_dir() {
            std::fs::remove_dir_all(&cleanup)
        } else {
            std::fs::remove_file(&cleanup)
        };
        if let Err(e) = removed {
            tracing::warn!(error = %e, "could not delete an export the hub refused to record");
        }
    }
    Response::from_parts(parts, axum::body::Body::from(bytes))
}
