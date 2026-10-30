//! `POST /v1/presign` (Story 8.1): a paid Hub with a device token gets 10-minute presigned PUT
//! URLs for keys under its own branch prefix of the data bucket: change-log batches, the branch
//! summary, daily rollups, snapshots and yearly GST archives.
//!
//! The tenant and the branch come from the token (hub row, then branch row); nothing of the kind
//! in the body is read. The cloud refuses Free branches, fences the summary, rollups and
//! snapshots of a Hub whose epoch is below the branch fence (a batch is still issued so no
//! change is lost), and limits each branch to a number of calls per hour.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, PRESIGN_KIND_GST_ARCHIVE, PRESIGN_KIND_KEYS, PRESIGN_KIND_ROLLUP,
    PRESIGN_KIND_SNAPSHOT, PresignRequest, PresignResponse, Tier, valid_date, valid_fy,
};
use arena_store::control::{HubRow, NewAdminAudit, RateRule};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::auth::AuthState;
use crate::presign::{
    Epoch, EpochError, MAX_SEQ, PutKind, batch_key, branch_prefix, gst_archive_key, has_path_chars,
    is_fenced, keys_key, rollup_key, snapshot_key, summary_key,
};

const RATE_PREFIX: &str = "presign:";
const WINDOW_MS: i64 = 60 * 60 * 1000;
/// Calls an hour are `LIMIT_BASIS_S / max(upload_interval_s, MIN_INTERVAL_S)`: four times the
/// Hub's cadence.
const LIMIT_BASIS_S: u64 = 4 * 3600;
const MIN_INTERVAL_S: u64 = 30;

/// Calls per hour a branch with this upload cadence may make.
pub fn hourly_limit(upload_interval_s: u32) -> u64 {
    LIMIT_BASIS_S / u64::from(upload_interval_s).max(MIN_INTERVAL_S)
}

/// What a valid request asks for.
#[derive(Debug, PartialEq, Eq)]
enum Want {
    Upload {
        batch: Option<(i64, i64)>,
        summary: bool,
    },
    Dated {
        kind: PutKind,
        date: String,
    },
    /// The player key backup (Story 9.2): one fixed key, no date.
    Keys,
    /// The yearly GST archive of a financial year (Story 9.14).
    GstArchive {
        fy: String,
    },
}

/// Why a request was not understood.
enum Refusal {
    /// A `/`, `\` or `..` in this field.
    Path(&'static str),
    Api(ApiError),
}

impl From<ApiError> for Refusal {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}

fn epoch_text() -> &'static str {
    "Send an epoch like 3:HUBINSTANCE (a number, a colon, then letters, digits, - or _)."
}

/// Checks the shape of `body`; nothing is issued or counted yet.
fn plan(body: &PresignRequest) -> Result<(Epoch, Want), Refusal> {
    let epoch = Epoch::parse(&body.epoch).map_err(|e| match e {
        EpochError::PathCharacters => Refusal::Path("epoch"),
        EpochError::Malformed => Refusal::Api(ApiError::validation("epoch", epoch_text())),
    })?;
    if let Some(kind) = body.kind.as_deref() {
        if body.batch.is_some() || body.summary {
            return Err(ApiError::validation(
                "kind",
                "Ask for a rollup, a snapshot or a GST archive on its own.",
            )
            .into());
        }
        let kind = match kind {
            PRESIGN_KIND_ROLLUP => PutKind::Rollup,
            PRESIGN_KIND_SNAPSHOT => PutKind::Snapshot,
            PRESIGN_KIND_KEYS => {
                if body.date.is_some() {
                    return Err(ApiError::validation("date", "A key backup has no date.").into());
                }
                return Ok((epoch, Want::Keys));
            }
            PRESIGN_KIND_GST_ARCHIVE => {
                if body.date.is_some() {
                    return Err(ApiError::validation("date", "A GST archive has no date.").into());
                }
                let fy = body.fy.as_deref().unwrap_or_default();
                if has_path_chars(fy) {
                    return Err(Refusal::Path("fy"));
                }
                if !valid_fy(fy) {
                    return Err(ApiError::validation(
                        "fy",
                        "Send a financial year like FY2026-27.",
                    )
                    .into());
                }
                return Ok((epoch, Want::GstArchive { fy: fy.to_owned() }));
            }
            _ => {
                return Err(ApiError::from(AppError::with_field(
                    ErrorCode::Unsupported,
                    "kind",
                    "This kind of upload isn't supported.",
                ))
                .into());
            }
        };
        let date = body.date.as_deref().unwrap_or_default();
        if has_path_chars(date) {
            return Err(Refusal::Path("date"));
        }
        if !valid_date(date) {
            return Err(ApiError::validation("date", "Send a date like 2026-10-29.").into());
        }
        return Ok((
            epoch,
            Want::Dated {
                kind,
                date: date.to_owned(),
            },
        ));
    }
    let batch = match body.batch {
        None => None,
        Some(b) => {
            if b.first < 0 || b.first > b.last || b.last > MAX_SEQ {
                return Err(ApiError::validation(
                    "batch",
                    "Send first and last sequence numbers, first not above last.",
                )
                .into());
            }
            Some((b.first, b.last))
        }
    };
    if batch.is_none() && !body.summary {
        return Err(ApiError::validation(
            "batch",
            "Ask for a batch, the summary, or a rollup or snapshot.",
        )
        .into());
    }
    Ok((
        epoch,
        Want::Upload {
            batch,
            summary: body.summary,
        },
    ))
}

fn fenced() -> ApiError {
    AppError::new(ErrorCode::Fenced, "A newer Hub has taken over this branch.").into()
}

/// `FORBIDDEN`, with an `admin_audit` row naming the Hub. The row is best effort: the refusal
/// stands if it cannot be written.
async fn path_refused(
    st: &AuthState,
    hub: &HubRow,
    tenant_id: &str,
    field: &str,
    now: i64,
) -> ApiError {
    tracing::warn!(hub_id = %hub.id, branch_id = %hub.branch_id, field, "presign: path characters refused");
    let summary = serde_json::json!({
        "hub_id": hub.id,
        "tenant_id": tenant_id,
        "branch_id": hub.branch_id,
        "field": field,
    })
    .to_string();
    let id = ulid::Ulid::generate().to_string();
    let request_id = ulid::Ulid::generate().to_string();
    if let Err(e) = st
        .db
        .append_admin_audit(&NewAdminAudit {
            id: &id,
            at: now,
            actor_kind: "system",
            actor_id: &hub.id,
            action: "presign.forbidden",
            target_kind: "branch",
            target_id: &hub.branch_id,
            reason: None,
            summary: &summary,
            request_id: &request_id,
        })
        .await
    {
        tracing::error!(error = %e, hub_id = %hub.id, "presign: audit row not written");
    }
    ApiError::forbidden("That request names a place outside this branch.")
}

async fn presign(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PresignRequest>,
) -> Result<Json<PresignResponse>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    let branch = st
        .db
        .branch(&hub.branch_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("hub without its branch"))?;
    let entitlement = st
        .db
        .entitlement(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let claims =
        crate::licence::claims_for(&branch.tenant_id, &branch.id, entitlement.as_ref(), now);
    if claims.tier == Tier::Free {
        return Err(AppError::new(
            ErrorCode::FeatureNotEntitled,
            "Uploading to the cloud needs a paid plan.",
        )
        .into());
    }
    if has_path_chars(&branch.tenant_id) || has_path_chars(&branch.id) {
        return Err(ApiError::internal(
            "tenant or branch id with path characters",
        ));
    }
    let (epoch, want) = match plan(&body) {
        Ok(p) => p,
        Err(Refusal::Api(e)) => return Err(e),
        Err(Refusal::Path(field)) => {
            return Err(path_refused(&st, &hub, &branch.tenant_id, field, now).await);
        }
    };
    let Some(presigner) = st.presigner.get() else {
        return Err(ApiError::unavailable(
            "no data bucket for presigned uploads",
        ));
    };
    let fence = st
        .db
        .branch_fence_epoch(&branch.id)
        .await
        .map_err(ApiError::internal)?;
    let stale = is_fenced(&epoch, fence.as_deref());

    let prefix = branch_prefix(&branch.tenant_id, &branch.id);
    let mut response = PresignResponse::default();
    let mut links: Vec<(PutKind, String)> = Vec::new();
    match &want {
        Want::Upload { batch, summary } => {
            if stale && batch.is_none() {
                return Err(fenced());
            }
            if let Some((first, last)) = batch {
                links.push((PutKind::Batch, batch_key(&prefix, &epoch, *first, *last)));
            }
            if *summary {
                if stale {
                    response.summary_fenced = Some(true);
                } else {
                    links.push((PutKind::Summary, summary_key(&prefix)));
                }
            }
        }
        Want::Keys => {
            if stale {
                return Err(fenced());
            }
            links.push((PutKind::Keys, keys_key(&prefix)));
        }
        Want::GstArchive { fy } => {
            if stale {
                return Err(fenced());
            }
            links.push((PutKind::GstArchive, gst_archive_key(&prefix, fy)));
        }
        Want::Dated { kind, date } => {
            if stale {
                return Err(fenced());
            }
            let key = match kind {
                PutKind::Rollup => rollup_key(&prefix, date),
                _ => snapshot_key(&prefix, date),
            };
            links.push((*kind, key));
        }
    }

    let key = format!("{RATE_PREFIX}{}:{}", branch.tenant_id, branch.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1,
        window_ms: WINDOW_MS,
        limit: hourly_limit(claims.upload_interval_s),
    };
    let reservation = match st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        Ok(r) => r,
        Err(wait_ms) => {
            st.put_metrics.refused(&branch.tenant_id, &branch.id);
            return Err(ApiError::rate_limited(ceil_s(wait_ms)));
        }
    };
    let mut urls = Vec::with_capacity(links.len());
    for (kind, key) in &links {
        match presigner.presign_put(key).await {
            Ok(url) => urls.push((*kind, url)),
            Err(e) => {
                // Nothing was handed out: give the hit back.
                if let Err(re) = st.db.rate_release(&reservation).await {
                    tracing::warn!(error = %re, "presign: rate hit not released");
                }
                return Err(ApiError::internal(e));
            }
        }
    }
    for (kind, url) in urls {
        st.put_metrics.issued(&branch.tenant_id, &branch.id, kind);
        match kind {
            PutKind::Batch => response.batch_url = Some(url),
            PutKind::Summary => response.summary_url = Some(url),
            PutKind::Rollup | PutKind::Snapshot | PutKind::Keys | PutKind::GstArchive => {
                response.url = Some(url);
            }
        }
    }
    Ok(Json(response))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/presign", post(presign))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arena_proto::PresignBatch;

    fn req(f: impl FnOnce(&mut PresignRequest)) -> PresignRequest {
        let mut r = PresignRequest {
            epoch: "3:HUB".into(),
            ..PresignRequest::default()
        };
        f(&mut r);
        r
    }

    fn api(r: Result<(Epoch, Want), Refusal>) -> (ErrorCode, Option<String>) {
        match r {
            Err(Refusal::Api(e)) => (e.body().code, e.body().field.clone()),
            Err(Refusal::Path(f)) => (ErrorCode::Forbidden, Some(f.to_owned())),
            Ok(_) => panic!("accepted"),
        }
    }

    #[test]
    fn the_hourly_limit_is_four_times_the_cadence_with_a_floor_of_30_seconds() {
        assert_eq!(hourly_limit(300), 48);
        assert_eq!(hourly_limit(900), 16);
        assert_eq!(hourly_limit(30), 480);
        assert_eq!(hourly_limit(5), 480);
        assert_eq!(hourly_limit(0), 480);
    }

    #[test]
    fn a_batch_and_a_summary_or_one_dated_file_is_understood() {
        let (e, w) = plan(&req(|r| {
            r.batch = Some(PresignBatch { first: 1, last: 9 });
            r.summary = true;
        }))
        .ok()
        .unwrap();
        assert_eq!(e.as_str(), "3:HUB");
        assert_eq!(
            w,
            Want::Upload {
                batch: Some((1, 9)),
                summary: true
            }
        );
        let (_, w) = plan(&req(|r| {
            r.kind = Some("snapshot".into());
            r.date = Some("2026-02-28".into());
        }))
        .ok()
        .unwrap();
        assert_eq!(
            w,
            Want::Dated {
                kind: PutKind::Snapshot,
                date: "2026-02-28".into()
            }
        );
    }

    #[test]
    fn bad_shapes_are_validation_failures_and_path_characters_are_forbidden() {
        let batch = |first, last| Some(PresignBatch { first, last });
        let v = |f| (ErrorCode::ValidationFailed, Some(f));
        assert_eq!(
            api(plan(&req(|r| r.epoch = String::new()))),
            v("epoch".into())
        );
        assert_eq!(api(plan(&req(|r| r.epoch = "x".into()))), v("epoch".into()));
        assert_eq!(api(plan(&req(|_| {}))), v("batch".into()), "nothing asked");
        assert_eq!(
            api(plan(&req(|r| r.batch = batch(5, 4)))),
            v("batch".into())
        );
        assert_eq!(
            api(plan(&req(|r| r.batch = batch(-1, 4)))),
            v("batch".into())
        );
        assert_eq!(
            api(plan(&req(|r| r.batch = batch(1, MAX_SEQ + 1)))),
            v("batch".into())
        );
        assert!(plan(&req(|r| r.batch = batch(MAX_SEQ, MAX_SEQ))).is_ok());
        for bad in ["2026-13-01", "2026-02-30", "26-01-01", "", "2026-1-1"] {
            assert_eq!(
                api(plan(&req(|r| {
                    r.kind = Some("rollup".into());
                    r.date = Some(bad.into());
                }))),
                v("date".into()),
                "{bad:?}"
            );
        }
        assert_eq!(
            api(plan(&req(|r| {
                r.kind = Some("rollup".into());
            }))),
            v("date".into()),
            "no date"
        );
        assert_eq!(
            api(plan(&req(|r| {
                r.kind = Some("state".into());
                r.date = Some("2026-01-01".into());
            }))),
            (ErrorCode::Unsupported, Some("kind".into()))
        );
        assert_eq!(
            api(plan(&req(|r| {
                r.kind = Some("rollup".into());
                r.date = Some("2026-01-01".into());
                r.summary = true;
            }))),
            v("kind".into())
        );
        assert_eq!(
            plan(&req(|r| {
                r.kind = Some("gst_archive".into());
                r.fy = Some("FY2026-27".into());
            }))
            .ok()
            .map(|(_, w)| w),
            Some(Want::GstArchive {
                fy: "FY2026-27".into()
            })
        );
        for bad in [
            None,
            Some(""),
            Some("FY2026-28"),
            Some("2026-27"),
            Some("FY26"),
        ] {
            assert_eq!(
                api(plan(&req(|r| {
                    r.kind = Some("gst_archive".into());
                    r.fy = bad.map(str::to_owned);
                }))),
                v("fy".into()),
                "{bad:?}"
            );
        }
        let forbidden = |f: &str| (ErrorCode::Forbidden, Some(f.to_owned()));
        for bad in ["../FY2026-27", "FY2026-27/x", r"FY2026\27", ".."] {
            assert_eq!(
                api(plan(&req(|r| {
                    r.kind = Some("gst_archive".into());
                    r.fy = Some(bad.into());
                }))),
                forbidden("fy"),
                "{bad:?}"
            );
        }
        for bad in ["../3:A", "3:A/B", "3:A\\B", "3:.."] {
            assert_eq!(
                api(plan(&req(|r| {
                    r.epoch = bad.into();
                    r.summary = true;
                }))),
                forbidden("epoch"),
                "{bad:?}"
            );
        }
        for bad in ["../../x", "2026-01-01/..", "2026\\01-01", "..", "a/b"] {
            assert_eq!(
                api(plan(&req(|r| {
                    r.kind = Some("snapshot".into());
                    r.date = Some(bad.into());
                }))),
                forbidden("date"),
                "{bad:?}"
            );
        }
    }
}
