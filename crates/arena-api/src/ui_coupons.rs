//! Coupons for the Hub (Story 5.21): `GET /ui/coupons` lists them for Settings → Coupons and
//! `POST /ui/coupons/preview` prices a code on a bill for the settle bar. Both only read: a
//! coupon is written by `coupon.upsert` and taken by the settle itself.

use std::sync::Arc;

use arena_proto::{
    AppError, CouponPreviewRequest, DiscountPreviewRequest, ErrorCode, Permission, SessionStaff,
    UiError,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bad_frame, bearer, fail, internal, ui_error};

const SHAPE: &str = "the body must be {\"code\": string} plus one of order_id, lines or plan_id";

async fn staff(
    auth: &UiAuth,
    headers: &HeaderMap,
    needs: Permission,
) -> Result<SessionStaff, UiError> {
    let token = bearer(headers).ok_or_else(auth_failed)?.to_owned();
    let me = auth.acting_session(&token).await?;
    if !me.permissions.contains(&needs) {
        return Err(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission for coupons",
        )));
    }
    Ok(me)
}

/// `GET /ui/coupons` (`catalog.edit`): every coupon, active or not, with its targets and uses.
pub(crate) async fn list(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    if let Err(e) = staff(&auth, &headers, Permission::CatalogEdit).await {
        return fail(e);
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.coupons()).await {
        Ok(Ok(list)) => Json(list).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `POST /ui/coupons/preview` (`pos.sell`): the discount and the total to pay if `code` is applied
/// to the bill the body names (an open bill, a counter cart or a plan). A coupon the bill cannot
/// take is `COUPON_INVALID` with the reason, and nothing changes.
pub(crate) async fn preview(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Ok(req) = serde_json::from_slice::<CouponPreviewRequest>(&body) else {
        return fail(bad_frame(SHAPE));
    };
    // A plan is sold by `plan.sell` (`wallet.topup`); every other bill by `pos.sell`.
    let needs = if req.plan_id.is_some() {
        Permission::WalletTopup
    } else {
        Permission::PosSell
    };
    if let Err(e) = staff(&auth, &headers, needs).await {
        return fail(e);
    }
    let (reader, now_ms) = (Arc::clone(auth.reader()), auth.clock().now_ms());
    match tokio::task::spawn_blocking(move || reader.coupon_preview(&req, now_ms)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

const DISCOUNT_SHAPE: &str = "the body must be {\"discount\": {...}, \"coupon\"?: string} plus one of order_id, lines or plan_id";

/// `POST /ui/discount/preview` (`pos.sell`; `wallet.topup` for a plan): the manual discount and the
/// total to pay if it is applied (after the coupon the body names) to the bill the body names.
/// Anyone who can sell may preview one; `discount.manual` is checked when the settle carries it. A
/// discount the bill cannot take is `VALIDATION_FAILED` on `discount`, and nothing changes.
pub(crate) async fn discount_preview(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Ok(req) = serde_json::from_slice::<DiscountPreviewRequest>(&body) else {
        return fail(bad_frame(DISCOUNT_SHAPE));
    };
    let needs = if req.plan_id.is_some() {
        Permission::WalletTopup
    } else {
        Permission::PosSell
    };
    if let Err(e) = staff(&auth, &headers, needs).await {
        return fail(e);
    }
    let (reader, now_ms) = (Arc::clone(auth.reader()), auth.clock().now_ms());
    match tokio::task::spawn_blocking(move || reader.discount_preview(&req, now_ms)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
