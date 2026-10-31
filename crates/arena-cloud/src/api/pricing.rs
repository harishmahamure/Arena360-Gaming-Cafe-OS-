//! `GET /v1/pricing?band=` (Story 10.1): the live price book's lines for one band, so the
//! wizard, the Owner app and the marketing site show the amounts that will be charged. Public:
//! it carries no tenant data. A band that is not on sale answers `BAND_NOT_AVAILABLE`.

use std::sync::Arc;

use arena_proto::{AppError, Band, ErrorCode};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::HeaderValue;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::error::ApiError;
use crate::auth::AuthState;
use crate::price_book::{self, PricingError};

#[derive(Deserialize)]
struct PricingQuery {
    band: Option<String>,
}

fn bad_band() -> ApiError {
    ApiError::bad_request("band must be IN, MY, SG or US.", Some("band".to_owned()))
}

async fn pricing(
    State(st): State<Arc<AuthState>>,
    query: Result<Query<PricingQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = query.map_err(|_| bad_band())?;
    let band = match q.band.as_deref() {
        None => Band::In,
        Some(b) => Band::parse(b).ok_or_else(bad_band)?,
    };
    match price_book::pricing(&st.db, band).await {
        Ok(body) => Ok((
            [(
                CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            )],
            Json(body),
        )
            .into_response()),
        Err(PricingError::BandNotAvailable(_)) => Err(ApiError::from(AppError::with_field(
            ErrorCode::BandNotAvailable,
            "band",
            "Paid plans aren't available in your country yet — Free is.",
        ))),
        Err(PricingError::NoLiveBook) => Err(ApiError::unavailable("no live price book")),
        Err(PricingError::Db(e)) => Err(ApiError::internal(e)),
    }
}

pub(super) fn router(st: Arc<AuthState>) -> Router {
    Router::new().route("/pricing", get(pricing)).with_state(st)
}
