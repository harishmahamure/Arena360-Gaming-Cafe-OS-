//! CSV import with a dry-run (Stories 5.33 and 5.34): `POST /ui/import/{products,players,plans}/
//! preview` reads a file against what the Hub holds and writes nothing; `.../commit` applies the
//! rows that passed as `import.*` intents of 500 rows, each with its own idempotency key derived
//! from the client's `import_id`, so a retry never duplicates; `.../template` saves the template.
//! Products and plans need `catalog.edit` (opening stock also needs `stock.in`, which `apply()`
//! checks); players need `player.manage` (balances also `wallet.topup`, limits `credit.manage`).

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use arena_core::{ChangeOp, is_ulid};
use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, IMPORT_CHUNK_ROWS, ImportCommitRequest, ImportPlans,
    ImportPlayers, ImportPreviewRequest, ImportProducts, ImportTemplateRequest,
    ImportTemplateSaved, Intent, IntentPayload, Permission, PlanImportResult, PlayerImportResult,
    ProductImportCommitRequest, ProductImportPreviewRequest, ProductImportResult, SessionStaff,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{
    UiAuth, auth_failed, bad_frame, bearer, fail, internal, player_kek, ui_error,
};
use crate::ui_report::write_export;

const PREVIEW_SHAPE: &str = "the body must be {\"csv\": string}";
const COMMIT_SHAPE: &str =
    "the body must be {\"csv\": string, \"import_id\": ULID, \"valid_only\"?: bool}";
const TEMPLATE_SHAPE: &str = "the body must be {\"folder\": string}";

/// Checks the bearer token and `catalog.edit`.
async fn gate(auth: &UiAuth, headers: &HeaderMap) -> Result<SessionStaff, Response> {
    gate_for(auth, headers, Permission::CatalogEdit, "products").await
}

/// Checks the bearer token and `permission`; `what` names what the import brings in.
async fn gate_for(
    auth: &UiAuth,
    headers: &HeaderMap,
    permission: Permission,
    what: &str,
) -> Result<SessionStaff, Response> {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return Err(fail(auth_failed()));
    };
    let me = auth.acting_session(&token).await.map_err(fail)?;
    if !me.permissions.contains(&permission) {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            format!("you do not have permission to import {what}"),
        ))));
    }
    Ok(me)
}

/// The ULID for chunk `n` of an import: the import's own ULID with its last 16 bits replaced by
/// the chunk number (`tag` keeps an intent id apart from its key), so the same `import_id` gives
/// the same chunk keys on a retry.
fn chunk_ulid(import_id: &str, n: usize, tag: u16) -> Option<String> {
    let base = ulid::Ulid::from_string(import_id).ok()?.0;
    let n = u16::try_from(n).ok()?;
    Some(ulid::Ulid((base & !0xFFFF) | u128::from(tag | n)).to_string())
}

/// `POST /ui/import/products/preview {csv}`: the dry-run.
pub(crate) async fn preview(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate(&auth, &headers).await {
        return denied;
    }
    let Ok(req) = serde_json::from_slice::<ProductImportPreviewRequest>(&body) else {
        return fail(bad_frame(PREVIEW_SHAPE));
    };
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    match tokio::task::spawn_blocking(move || reader.product_import(&req.csv, now)).await {
        Ok(Ok((preview, _))) => Json(preview).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/import/products/commit {csv, import_id, valid_only?}`: applies the file. Without
/// `valid_only` a file with any problem is refused (`VALIDATION_FAILED` on `csv`).
pub(crate) async fn commit(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let me = match gate(&auth, &headers).await {
        Ok(me) => me,
        Err(denied) => return denied,
    };
    let Ok(req) = serde_json::from_slice::<ProductImportCommitRequest>(&body) else {
        return fail(bad_frame(COMMIT_SHAPE));
    };
    if !is_ulid(&req.import_id) {
        return fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            "import_id",
            "import_id must be a ULID",
        )));
    }
    let Some(writer) = auth.writer() else {
        return fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        )));
    };
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    let csv = req.csv.clone();
    let read = tokio::task::spawn_blocking(move || reader.product_import(&csv, now)).await;
    let (preview, rows) = match read {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    let refuse = |message: &str| {
        fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            "csv",
            message,
        )))
    };
    if preview.unusable || rows.is_empty() {
        return refuse("the file has no rows that can be imported");
    }
    if !req.valid_only && !preview.can_commit {
        return refuse("the file has problems; fix it, or import the valid rows only");
    }
    let (mut created, mut updated, mut chunks) = (0i64, 0i64, 0i64);
    for (n, chunk) in rows.chunks(IMPORT_CHUNK_ROWS).enumerate() {
        let (Some(key), Some(id)) = (
            chunk_ulid(&req.import_id, n, 0),
            chunk_ulid(&req.import_id, n, 0x8000),
        ) else {
            return refuse("the file has too many rows");
        };
        let payload = IntentPayload::ImportProducts(ImportProducts {
            rows: chunk.to_vec(),
        });
        let intent = Intent {
            intent_id: id,
            kind: payload.kind(),
            idempotency_key: key,
            issued_at: now,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::Staff,
                id: me.staff_id.clone(),
            },
            elevated_by: None,
            reason: None,
            payload,
        };
        match writer.submit_intent(intent).await {
            Ok(applied) => {
                chunks += 1;
                for c in applied.changes.iter().filter(|c| c.tbl == "products") {
                    match c.op {
                        ChangeOp::Insert => created += 1,
                        ChangeOp::Update => updated += 1,
                        _ => {}
                    }
                }
            }
            Err(e) => {
                tracing::warn!(chunk = n, applied = chunks, error = %e, "product import stopped");
                return fail(ui_error(e));
            }
        }
    }
    Json(ProductImportResult {
        created,
        updated,
        skipped: preview.failed,
        chunks,
    })
    .into_response()
}

/// `POST /ui/import/products/template {folder}`: saves the template CSV into `folder`.
pub(crate) async fn template(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate(&auth, &headers).await {
        return denied;
    }
    let Ok(req) = serde_json::from_slice::<ImportTemplateRequest>(&body) else {
        return fail(bad_frame(TEMPLATE_SHAPE));
    };
    let job = tokio::task::spawn_blocking(move || {
        let file_name = arena_core::import_products::TEMPLATE_FILE;
        let path = write_export(
            &req.folder,
            file_name,
            arena_core::import_products::template().as_bytes(),
        )?;
        Ok::<_, AppError>(ImportTemplateSaved {
            file_name: file_name.to_owned(),
            path: path.to_string_lossy().into_owned(),
        })
    })
    .await;
    match job {
        Ok(Ok(saved)) => Json(saved).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

// ----------------------------------------------------------------------------------------------
// Players and plans (Story 5.34)
// ----------------------------------------------------------------------------------------------

/// Sends each payload as the next chunk of the import, with keys derived from `import_id`, and
/// answers what each applied. Stops at the first chunk the Hub refuses.
async fn submit_chunks(
    auth: &UiAuth,
    me: &SessionStaff,
    import_id: &str,
    now: i64,
    payloads: Vec<IntentPayload>,
) -> Result<Vec<arena_core::Applied>, Response> {
    let Some(writer) = auth.writer() else {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::NotPrimary,
            "this hub is not the primary writer",
        ))));
    };
    let mut applied = Vec::new();
    for (n, payload) in payloads.into_iter().enumerate() {
        let (Some(key), Some(id)) = (
            chunk_ulid(import_id, n, 0),
            chunk_ulid(import_id, n, 0x8000),
        ) else {
            return Err(commit_refused("the file has too many rows"));
        };
        let intent = Intent {
            intent_id: id,
            kind: payload.kind(),
            idempotency_key: key,
            issued_at: now,
            base_rev: None,
            actor: Actor {
                kind: ActorKind::Staff,
                id: me.staff_id.clone(),
            },
            elevated_by: None,
            reason: None,
            payload,
        };
        match writer.submit_intent(intent).await {
            Ok(done) => applied.push(done),
            Err(e) => {
                tracing::warn!(chunk = n, applied = applied.len(), error = %e, "import stopped");
                return Err(fail(ui_error(e)));
            }
        }
    }
    Ok(applied)
}

fn commit_refused(message: &str) -> Response {
    fail(ui_error(AppError::with_field(
        ErrorCode::ValidationFailed,
        "csv",
        message,
    )))
}

/// How many rows of `table` the chunks inserted and updated.
fn counted(applied: &[arena_core::Applied], table: &str) -> (i64, i64) {
    let (mut inserted, mut updated) = (0, 0);
    for c in applied
        .iter()
        .flat_map(|a| &a.changes)
        .filter(|c| c.tbl == table)
    {
        match c.op {
            ChangeOp::Insert => inserted += 1,
            ChangeOp::Update => updated += 1,
            _ => {}
        }
    }
    (inserted, updated)
}

/// Sealed chunks of recent player imports, by `import_id` and a hash of the file. Sealing makes
/// a fresh key and nonce per row, so a retry has to send the very same chunks to be recognised
/// by its idempotency keys; the next attempt with the same id and file takes them from here.
struct SealedImport {
    import_id: String,
    file: u64,
    chunks: Vec<ImportPlayers>,
    matched: i64,
    failed: i64,
}

const SEALED_KEPT: usize = 4;

fn sealed_imports() -> &'static Mutex<VecDeque<SealedImport>> {
    static KEPT: OnceLock<Mutex<VecDeque<SealedImport>>> = OnceLock::new();
    KEPT.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn file_hash(csv: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    csv.hash(&mut h);
    h.finish()
}

/// `POST /ui/import/players/preview {csv}`: the dry-run.
pub(crate) async fn players_preview(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate_for(&auth, &headers, Permission::PlayerManage, "players").await {
        return denied;
    }
    let Ok(req) = serde_json::from_slice::<ImportPreviewRequest>(&body) else {
        return fail(bad_frame(PREVIEW_SHAPE));
    };
    let kek = match player_kek(&auth) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    let read =
        tokio::task::spawn_blocking(move || reader.player_import(&req.csv, now, &kek.kek()?)).await;
    match read {
        Ok(Ok((preview, _, _))) => Json(preview).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/import/players/commit {csv, import_id, valid_only?}`: creates the players the file
/// adds, in chunks of 500. Without `valid_only` a file with any problem is refused
/// (`VALIDATION_FAILED` on `csv`); players whose phone already has a profile are left alone.
pub(crate) async fn players_commit(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let me = match gate_for(&auth, &headers, Permission::PlayerManage, "players").await {
        Ok(me) => me,
        Err(denied) => return denied,
    };
    let Ok(req) = serde_json::from_slice::<ImportCommitRequest>(&body) else {
        return fail(bad_frame(COMMIT_SHAPE));
    };
    if !is_ulid(&req.import_id) {
        return fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            "import_id",
            "import_id must be a ULID",
        )));
    }
    let kek = match player_kek(&auth) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    let now = auth.clock().now_ms();
    let file = file_hash(&req.csv);
    let kept = sealed_imports().lock().ok().and_then(|k| {
        k.iter()
            .find(|s| s.import_id == req.import_id && s.file == file)
            .map(|s| (s.chunks.clone(), s.matched, s.failed))
    });
    let (chunks, matched, failed) = match kept {
        Some(found) => found,
        None => {
            let reader = Arc::clone(auth.reader());
            let csv = req.csv.clone();
            let built = tokio::task::spawn_blocking(move || {
                let kek = kek.kek()?;
                let (preview, lines, mut taken) = reader.player_import(&csv, now, &kek)?;
                let sealed = arena_core::import_players::seal_rows(&kek, &lines, &mut taken)?;
                Ok::<_, AppError>((preview, sealed))
            })
            .await;
            let (preview, sealed) = match built {
                Ok(Ok(found)) => found,
                Ok(Err(e)) => return fail(ui_error(e)),
                Err(_) => return fail(internal()),
            };
            if preview.unusable || sealed.is_empty() {
                return commit_refused(if preview.matched > 0 && !preview.unusable {
                    "every player in the file already has a profile"
                } else {
                    "the file has no rows that can be imported"
                });
            }
            if !req.valid_only && !preview.can_commit {
                return commit_refused(
                    "the file has problems; fix it, or import the valid rows only",
                );
            }
            let chunks: Vec<ImportPlayers> = sealed
                .chunks(IMPORT_CHUNK_ROWS)
                .map(|c| ImportPlayers { rows: c.to_vec() })
                .collect();
            if let Ok(mut k) = sealed_imports().lock() {
                if k.len() >= SEALED_KEPT {
                    k.pop_front();
                }
                k.push_back(SealedImport {
                    import_id: req.import_id.clone(),
                    file,
                    chunks: chunks.clone(),
                    matched: preview.matched,
                    failed: preview.failed,
                });
            }
            (chunks, preview.matched, preview.failed)
        }
    };
    let payloads = chunks
        .into_iter()
        .map(IntentPayload::ImportPlayers)
        .collect();
    let applied = match submit_chunks(&auth, &me, &req.import_id, now, payloads).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    Json(PlayerImportResult {
        created: counted(&applied, "players").0,
        matched,
        skipped: failed,
        chunks: i64::try_from(applied.len()).unwrap_or(i64::MAX),
    })
    .into_response()
}

/// `POST /ui/import/players/template {folder}`: saves the template CSV into `folder`.
pub(crate) async fn players_template(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate_for(&auth, &headers, Permission::PlayerManage, "players").await {
        return denied;
    }
    save_template(
        &body,
        arena_core::import_players::TEMPLATE_FILE,
        arena_core::import_players::template(),
    )
    .await
}

/// `POST /ui/import/plans/preview {csv}`: the dry-run.
pub(crate) async fn plans_preview(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate_for(&auth, &headers, Permission::CatalogEdit, "plans").await {
        return denied;
    }
    let Ok(req) = serde_json::from_slice::<ImportPreviewRequest>(&body) else {
        return fail(bad_frame(PREVIEW_SHAPE));
    };
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    match tokio::task::spawn_blocking(move || reader.plan_import(&req.csv, now)).await {
        Ok(Ok((preview, _))) => Json(preview).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/import/plans/commit {csv, import_id, valid_only?}`: creates or updates the plans of
/// the file in chunks of 500. Without `valid_only` a file with any problem is refused
/// (`VALIDATION_FAILED` on `csv`).
pub(crate) async fn plans_commit(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let me = match gate_for(&auth, &headers, Permission::CatalogEdit, "plans").await {
        Ok(me) => me,
        Err(denied) => return denied,
    };
    let Ok(req) = serde_json::from_slice::<ImportCommitRequest>(&body) else {
        return fail(bad_frame(COMMIT_SHAPE));
    };
    if !is_ulid(&req.import_id) {
        return fail(ui_error(AppError::with_field(
            ErrorCode::ValidationFailed,
            "import_id",
            "import_id must be a ULID",
        )));
    }
    let reader = Arc::clone(auth.reader());
    let now = auth.clock().now_ms();
    let csv = req.csv.clone();
    let read = tokio::task::spawn_blocking(move || reader.plan_import(&csv, now)).await;
    let (preview, rows) = match read {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    if preview.unusable || rows.is_empty() {
        return commit_refused("the file has no rows that can be imported");
    }
    if !req.valid_only && !preview.can_commit {
        return commit_refused("the file has problems; fix it, or import the valid rows only");
    }
    let payloads = rows
        .chunks(IMPORT_CHUNK_ROWS)
        .map(|c| IntentPayload::ImportPlans(ImportPlans { rows: c.to_vec() }))
        .collect();
    let applied = match submit_chunks(&auth, &me, &req.import_id, now, payloads).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let (created, updated) = counted(&applied, "plans");
    Json(PlanImportResult {
        created,
        updated,
        skipped: preview.failed,
        chunks: i64::try_from(applied.len()).unwrap_or(i64::MAX),
    })
    .into_response()
}

/// `POST /ui/import/plans/template {folder}`: saves the template CSV into `folder`.
pub(crate) async fn plans_template(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Err(denied) = gate_for(&auth, &headers, Permission::CatalogEdit, "plans").await {
        return denied;
    }
    save_template(
        &body,
        arena_core::import_plans::TEMPLATE_FILE,
        arena_core::import_plans::template(),
    )
    .await
}

/// Saves `text` as `file_name` in the folder the request names.
async fn save_template(body: &[u8], file_name: &'static str, text: String) -> Response {
    let Ok(req) = serde_json::from_slice::<ImportTemplateRequest>(body) else {
        return fail(bad_frame(TEMPLATE_SHAPE));
    };
    let job = tokio::task::spawn_blocking(move || {
        let path = write_export(&req.folder, file_name, text.as_bytes())?;
        Ok::<_, AppError>(ImportTemplateSaved {
            file_name: file_name.to_owned(),
            path: path.to_string_lossy().into_owned(),
        })
    })
    .await;
    match job {
        Ok(Ok(saved)) => Json(saved).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
