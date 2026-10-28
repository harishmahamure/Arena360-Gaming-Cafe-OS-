//! CSV import with a dry-run (Story 5.33): `POST /ui/import/products/preview` reads a file
//! against the products the Hub holds and writes nothing; `.../commit` applies the rows that
//! passed as `import.products` intents of 500 rows, each with its own idempotency key derived
//! from the client's `import_id`, so a retry never duplicates; `.../template` saves the template.
//! All three need `catalog.edit`; opening stock also needs `stock.in`, which `apply()` checks.

use std::sync::Arc;

use arena_core::{ChangeOp, is_ulid};
use arena_proto::{
    Actor, ActorKind, AppError, ErrorCode, IMPORT_CHUNK_ROWS, ImportProducts,
    ImportTemplateRequest, ImportTemplateSaved, Intent, IntentPayload, Permission,
    ProductImportCommitRequest, ProductImportPreviewRequest, ProductImportResult, SessionStaff,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bad_frame, bearer, fail, internal, ui_error};
use crate::ui_report::write_export;

const PREVIEW_SHAPE: &str = "the body must be {\"csv\": string}";
const COMMIT_SHAPE: &str =
    "the body must be {\"csv\": string, \"import_id\": ULID, \"valid_only\"?: bool}";
const TEMPLATE_SHAPE: &str = "the body must be {\"folder\": string}";

/// Checks the bearer token and `catalog.edit`.
async fn gate(auth: &UiAuth, headers: &HeaderMap) -> Result<SessionStaff, Response> {
    let Some(token) = bearer(headers).map(str::to_owned) else {
        return Err(fail(auth_failed()));
    };
    let me = auth.acting_session(&token).await.map_err(fail)?;
    if !me.permissions.contains(&Permission::CatalogEdit) {
        return Err(fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission to import products",
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
