//! The Refund screen's lookup (Story 5.24): `GET /ui/refund/invoice?no=<invoice number>` opens a
//! settled invoice with its lines, what each has had refunded and the credit notes already issued.
//! It only reads: the refund itself is the `refund.create` intent, which asks for `refund.issue`
//! (a Counter elevates with a manager PIN).

use std::sync::Arc;

use arena_proto::{AppError, ErrorCode, Permission};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, internal, ui_error};

#[derive(Deserialize)]
pub(crate) struct InvoiceQuery {
    no: String,
}

/// `GET /ui/refund/invoice?no=` (`pos.sell`): the invoice, or `INVOICE_NOT_FOUND`.
pub(crate) async fn invoice(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Query(q): Query<InvoiceQuery>,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    let me = match auth.acting_session(&token).await {
        Ok(me) => me,
        Err(e) => return fail(e),
    };
    if !me.permissions.contains(&Permission::PosSell) {
        return fail(ui_error(AppError::new(
            ErrorCode::PermissionDenied,
            "you do not have permission for refunds",
        )));
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.refund_invoice(&q.no)).await {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}
