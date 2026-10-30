//! `POST /v1/restore/plan` (Story 9.12, AD-36): a replacement Hub that was released the branch key
//! gets 10-minute read URLs for the branch's cloud backup.
//!
//! The tenant and the branch come from the Hub token; nothing of the kind in the body is read, and
//! only keys under that branch's own prefix are listed or signed. Checks, in order: the token,
//! the Hub's own instance, a paid tier (`FEATURE_NOT_ENTITLED`), a Hub that replaces another one
//! (`FORBIDDEN`), the owner's key released to this instance (`KEY_NOT_RELEASED`), and a limit of
//! calls an hour per branch.
//!
//! The cloud keeps no manifest and a snapshot's last change is inside the file, so a request
//! without `after` answers the newest snapshot and the key backup and says the batches are
//! pending; the Hub asks again with the position it read from the snapshot.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, PRESIGN_TTL_S, RestorePlan, RestorePlanBatch, RestorePlanRequest,
    RestorePlanSnapshot, RestorePosition, Tier, is_rollup_date,
};
use arena_store::control::RateRule;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};

use super::error::ApiError;
use super::heartbeat::authenticate;
use super::json::ApiJson;
use super::signup::ceil_s;
use crate::audit_build::{BatchKey, batch_keys, epoch_order};
use crate::auth::AuthState;
use crate::presign::{branch_prefix, has_path_chars, keys_key};

const RATE_PREFIX: &str = "restoreplan:";
const WINDOW_MS: i64 = 60 * 60 * 1000;
/// Plans an hour per branch: two per attempt plus a refresh every 10 minutes, with room to spare.
const HOURLY_LIMIT: u64 = 60;

/// The newest `snapshots/<date>.db.zst` in `keys`: `(date, key)`.
fn newest_snapshot(prefix: &str, keys: &[String]) -> Option<(String, String)> {
    let root = format!("{prefix}snapshots/");
    keys.iter()
        .filter_map(|k| {
            let date = k.strip_prefix(&root)?.strip_suffix(".db.zst")?;
            is_rollup_date(date).then(|| (date.to_owned(), k.clone()))
        })
        .max()
}

/// The batches after `after` (all of them without one), in log order.
fn batches_after(prefix: &str, keys: &[String], after: Option<&RestorePosition>) -> Vec<BatchKey> {
    batch_keys(prefix, keys)
        .into_iter()
        .filter(|b| match after {
            None => true,
            Some(p) => match epoch_order(&b.epoch).cmp(&epoch_order(&p.epoch)) {
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => b.last > p.seq,
                std::cmp::Ordering::Greater => true,
            },
        })
        .collect()
}

async fn plan(
    State(st): State<Arc<AuthState>>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RestorePlanRequest>,
) -> Result<Json<RestorePlan>, ApiError> {
    let now = st.now();
    let hub = authenticate(&st, &headers, now).await?;
    if body.hub_instance_id != hub.hub_instance_id {
        return Err(ApiError::validation(
            "hub_instance_id",
            "That is not this Hub's instance.",
        ));
    }
    if let Some(after) = &body.after
        && (after.seq < 0 || epoch_order(&after.epoch).is_none())
    {
        return Err(ApiError::validation(
            "after",
            "Send the snapshot's last change as an epoch like 3:HUBINSTANCE and a seq.",
        ));
    }
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
            "Restoring from the cloud needs a paid plan.",
        )
        .into());
    }
    if !st
        .db
        .branch_has_history(&hub.branch_id, &hub.hub_instance_id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::forbidden(
            "This Hub does not replace another Hub of the branch.",
        ));
    }
    if !st
        .db
        .key_released_to(&hub.branch_id, &hub.hub_instance_id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(AppError::new(
            ErrorCode::KeyNotReleased,
            "The owner has not released the branch key to this Hub.",
        )
        .into());
    }
    if has_path_chars(&branch.tenant_id) || has_path_chars(&branch.id) {
        return Err(ApiError::internal(
            "tenant or branch id with path characters",
        ));
    }
    let Some(presigner) = st.presigner.get() else {
        return Err(ApiError::unavailable("no data bucket for restore links"));
    };
    let key = format!("{RATE_PREFIX}{}", branch.id);
    let rule = RateRule {
        key: &key,
        prune_prefix: RATE_PREFIX,
        bucket_ms: 1,
        window_ms: WINDOW_MS,
        limit: HOURLY_LIMIT,
    };
    if let Err(wait_ms) = st
        .db
        .rate_reserve(&[rule], now)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::rate_limited(ceil_s(wait_ms)));
    }

    let prefix = branch_prefix(&branch.tenant_id, &branch.id);
    let snapshots = presigner
        .list(&format!("{prefix}snapshots/"))
        .await
        .map_err(ApiError::internal)?;
    let snapshot = newest_snapshot(&prefix, &snapshots);
    let mut out = RestorePlan {
        expires_in_s: PRESIGN_TTL_S,
        ..RestorePlan::default()
    };
    if let Some((date, key)) = &snapshot {
        out.snapshot = Some(RestorePlanSnapshot {
            date: date.clone(),
            url: presigner
                .presign_get(key)
                .await
                .map_err(ApiError::internal)?,
        });
    }
    let backup_key = keys_key(&prefix);
    let stored = presigner
        .list(&backup_key)
        .await
        .map_err(ApiError::internal)?;
    if stored.contains(&backup_key) {
        out.keys_url = Some(
            presigner
                .presign_get(&backup_key)
                .await
                .map_err(ApiError::internal)?,
        );
    }
    if snapshot.is_some() && body.after.is_none() {
        out.batches_pending = true;
    } else {
        let listed = presigner
            .list(&format!("{prefix}changes/"))
            .await
            .map_err(ApiError::internal)?;
        for b in batches_after(&prefix, &listed, body.after.as_ref()) {
            out.batches.push(RestorePlanBatch {
                epoch: b.epoch,
                first: b.first,
                last: b.last,
                url: presigner
                    .presign_get(&b.key)
                    .await
                    .map_err(ApiError::internal)?,
            });
        }
    }
    tracing::info!(
        hub_id = %hub.id,
        branch_id = %hub.branch_id,
        snapshot = out.snapshot.is_some(),
        batches = out.batches.len(),
        "restore plan issued"
    );
    Ok(Json(out))
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new()
        .route("/restore/plan", post(plan))
        .with_state(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "tenants/T/branches/B/";

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_newest_snapshot_is_the_latest_date_under_the_prefix() {
        let list = keys(&[
            "tenants/T/branches/B/snapshots/2026-10-01.db.zst",
            "tenants/T/branches/B/snapshots/2026-10-29.db.zst",
            "tenants/T/branches/B/snapshots/notes.txt",
            "tenants/T/branches/X/snapshots/2026-12-01.db.zst",
        ]);
        let (date, key) = newest_snapshot(P, &list).unwrap();
        assert_eq!(date, "2026-10-29");
        assert!(key.ends_with("2026-10-29.db.zst") && key.starts_with(P));
        assert_eq!(newest_snapshot(P, &[]), None);
    }

    #[test]
    fn batches_after_a_position_cross_epochs_in_log_order() {
        let list = keys(&[
            "tenants/T/branches/B/changes/2:H/000000000001-000000000005.zst",
            "tenants/T/branches/B/changes/1:H/000000000001-000000000009.zst",
            "tenants/T/branches/B/changes/1:H/000000000010-000000000019.zst",
            "tenants/T/branches/B/changes/10:H/000000000001-000000000002.zst",
            "tenants/T/branches/X/changes/1:H/000000000020-000000000029.zst",
        ]);
        let at = |epoch: &str, seq| RestorePosition {
            epoch: epoch.into(),
            seq,
        };
        let got = |after: Option<&RestorePosition>| -> Vec<(String, i64)> {
            batches_after(P, &list, after)
                .into_iter()
                .map(|b| (b.epoch, b.first))
                .collect()
        };
        assert_eq!(got(None).len(), 4, "foreign prefixes never count");
        assert_eq!(
            got(Some(&at("1:H", 9))),
            [("1:H".into(), 10), ("2:H".into(), 1), ("10:H".into(), 1)]
        );
        assert_eq!(
            got(Some(&at("1:H", 5))).first(),
            Some(&("1:H".to_owned(), 1)),
            "a batch that ends after the position is still needed"
        );
        assert_eq!(got(Some(&at("2:H", 5))), [("10:H".into(), 1)]);
    }
}
