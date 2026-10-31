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

// ---- restore from a file (Story 9.7) ----

/// The answer for an `AppError` of a restore route: a refused file is 422 (including a newer
/// schema, which the general map answers 500), anything else as elsewhere.
fn restore_fail(e: AppError) -> Response {
    let status = match e.code {
        ErrorCode::RestoreWrongBranch | ErrorCode::RestoreCorrupt | ErrorCode::SchemaTooNew => {
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        }
        other => crate::ui_auth::status_of(other),
    };
    (status, Json(ui_error(e))).into_response()
}

fn path_error(reason: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, "path", reason)
}

/// The file the Owner named: set, and absolute (a service has no useful working directory).
fn restore_path(path: &str) -> Result<PathBuf, AppError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(path_error("not_set"));
    }
    if arena_proto::is_backup_tree_uri(path) {
        return Err(path_error("unsupported"));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(path_error("not_absolute"));
    }
    Ok(path)
}

/// A Windows Hub only: the Android Hub cannot restart its own process (Story 9.13 and Epic 10).
fn restore_supported(auth: &UiAuth) -> Result<(), AppError> {
    let android = auth
        .status()
        .is_some_and(|s| s.borrow().platform == arena_proto::HubPlatform::Android);
    if android {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "restoring from a file needs the Windows Hub",
        ));
    }
    Ok(())
}

fn not_primary() -> AppError {
    AppError::new(ErrorCode::NotPrimary, "this hub is not the primary writer")
}

/// `POST /ui/backup/restore/check {path, pin?}` answers a [`arena_proto::RestoreCheck`]: what the
/// file holds and what a restore of it would set aside. Writes nothing. The Owner checks with
/// their session; any other member must give the Owner's PIN.
pub(crate) async fn restore_check(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(request) = serde_json::from_slice::<arena_proto::RestoreCheckRequest>(&body) else {
        return fail(bad_frame("the body must be {\"path\", \"pin\"?}"));
    };
    if let Err(e) = restore_supported(&auth) {
        return restore_fail(e);
    }
    if auth.writer().is_none() {
        return restore_fail(not_primary());
    }
    if !(me.permissions.contains(&Permission::OpsRestore) && request.pin.is_none())
        && let Err(e) = auth.restore_owner(&me, request.pin.clone()).await
    {
        return fail(e);
    }
    let path = match restore_path(&request.path) {
        Ok(p) => p,
        Err(e) => return restore_fail(e),
    };
    let now = auth.clock().now_ms();
    let reader = Arc::clone(auth.reader());
    let made = tokio::task::spawn_blocking(move || {
        let found = reader.inspect_restore(&path)?;
        let tz = reader
            .locale(now)?
            .map_or_else(|| "UTC".to_owned(), |l| l.tz);
        let code = reader.branch()?.map(|b| b.branch_code).unwrap_or_default();
        Ok::<_, AppError>((found, tz, code, path))
    })
    .await;
    let (found, tz, branch_code, path) = match made {
        Ok(Ok(done)) => done,
        Ok(Err(e)) => return restore_fail(e),
        Err(_) => return fail(internal()),
    };
    let free = auth
        .status()
        .is_some_and(|s| s.borrow().tier == Some(arena_proto::Tier::Free));
    let erased_may_reappear = free && !found.has_tombstones;
    let header = found.header;
    Json(arena_proto::RestoreCheck {
        branch_name: found.branch_name,
        branch_code,
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bytes: i64::try_from(found.file_bytes).unwrap_or(i64::MAX),
        exported_at: header.exported_at,
        epoch: header.epoch,
        last_seq: header.last_seq,
        schema_version: header.schema_version,
        rows_set_aside: found.rows_set_aside,
        warning: arena_proto::restore_warning(header.exported_at, &tz, erased_may_reappear),
        erased_may_reappear,
    })
    .into_response()
}

/// `POST /ui/backup/restore {path, pin, branch_code}` (Story 9.7): checks the Owner's PIN and the
/// typed branch code, checks the file again, stages it and stops the Hub. The restore itself runs
/// at the next start, before the writer opens. A refusal stages nothing.
pub(crate) async fn restore(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    let Ok(request) = serde_json::from_slice::<arena_proto::RestoreRequest>(&body) else {
        return fail(bad_frame(
            "the body must be {\"path\", \"pin\", \"branch_code\"}",
        ));
    };
    if let Err(e) = restore_supported(&auth) {
        return restore_fail(e);
    }
    if auth.writer().is_none() {
        return restore_fail(not_primary());
    }
    let owner = match auth.restore_owner(&me, Some(request.pin.clone())).await {
        Ok(owner) => owner,
        Err(e) => return fail(e),
    };
    let reader = Arc::clone(auth.reader());
    let code = {
        let reader = Arc::clone(&reader);
        match tokio::task::spawn_blocking(move || reader.branch()).await {
            Ok(Ok(branch)) => branch.map(|b| b.branch_code).unwrap_or_default(),
            Ok(Err(e)) => return fail(ui_error(e)),
            Err(_) => return fail(internal()),
        }
    };
    if code.is_empty() || request.branch_code.trim() != code {
        return restore_fail(AppError::with_field(
            ErrorCode::ValidationFailed,
            "branch_code",
            "the branch code does not match",
        ));
    }
    let path = match restore_path(&request.path) {
        Ok(p) => p,
        Err(e) => return restore_fail(e),
    };

    // One disk-heavy job at a time, and one staged restore.
    let _single = auth.export_lock().lock().await;
    let staged = tokio::task::spawn_blocking(move || reader.stage_restore(&path, &owner)).await;
    let found = match staged {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => return restore_fail(e),
        Err(_) => return fail(internal()),
    };
    tracing::warn!(
        staff_id = %me.staff_id,
        last_seq = found.header.last_seq,
        rows_set_aside = found.rows_set_aside,
        "restore staged; the hub is restarting"
    );
    if let Some(restart) = auth.restart_hook() {
        // Let the answer reach the browser first.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            restart();
        });
    }
    Json(arena_proto::RestoreStaged { restarting: true }).into_response()
}

/// `GET /ui/backup/status` (any signed-in member, Story 9.16): how far the cloud backup has got,
/// for the header ("Backed up 4 min ago") and System -> Backup. It reads the Hub's own logs, so
/// it is the same whether or not the cloud can be reached now. A Free Hub answers `cloud: false`
/// and nothing else. The interval is the tier's default: the licence may set its own, but the
/// header's "behind" mark only needs the order of magnitude.
pub(crate) async fn status(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.acting_session(&token).await {
        return fail(e);
    }
    let tier = auth
        .status()
        .and_then(|s| s.borrow().tier)
        .filter(|t| *t != arena_proto::Tier::Free);
    let interval = tier.map_or(0, |t| t.defaults().upload_interval_s);
    let now = auth.clock().now_ms();
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.backup_status(tier.is_some(), interval, now))
        .await
    {
        Ok(Ok(status)) => Json(status).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `GET /ui/backup/gst-archive` (`report.view`, Story 9.14): the financial years whose GST archive
/// the Hub has uploaded, newest first, for System -> Backup ("GST archive 26-27 saved"). Reads the
/// `gst_archive_log` rows only; a Free Hub has none.
pub(crate) async fn gst_archives(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::ReportView) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission to view the backup",
        )));
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.gst_archives()).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

// ---- what a promote or restore set aside (Story 9.23) ----

/// `GET /ui/backup/set-aside` (`report.view`): the forks this Hub's epoch came from and, from every
/// database in `orphaned/` on this PC, the changes set aside and the invoice numbers only those
/// databases hold, for System -> Backup -> "Set aside after promote or restore". Answers a
/// [`arena_proto::SetAsideList`]; reads only.
pub(crate) async fn set_aside(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::ReportView) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission to view what was set aside",
        )));
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.set_aside()).await {
        Ok(Ok(found)) => Json(found.list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/backup/set-aside/export` (`backup.export`): "Export list" writes the same list as CSV
/// (every orphaned invoice, not only the ones the screen shows) to the backup location as
/// `<branch_code>-set-aside-<yyyy-mm-dd-hhmm>.csv`. Answers a [`arena_proto::SetAsideExported`].
/// `VALIDATION_FAILED` on `folder` says why the location cannot take it. The list is a copy of
/// what the Hub already holds, so nothing is recorded.
pub(crate) async fn set_aside_export(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::BackupExport) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "exporting the list needs permission to export the database",
        )));
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
    let now = auth.clock().now_ms();
    match tokio::task::spawn_blocking(move || reader.export_set_aside(&folder, now)).await {
        Ok(Ok((done, path))) => {
            tracing::info!(
                staff_id = %me.staff_id,
                file = %done.file_name,
                bytes = done.bytes,
                path = %path.display(),
                "set-aside list exported"
            );
            Json(done).into_response()
        }
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
