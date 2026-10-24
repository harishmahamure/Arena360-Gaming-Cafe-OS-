use arena_proto::{COUNTRIES, Country};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};

const CACHE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "public, max-age=86400")];

async fn countries() -> impl IntoResponse {
    let list: Vec<Country> = COUNTRIES.iter().map(Country::from).collect();
    (CACHE, Json(list))
}

async fn timezones() -> impl IntoResponse {
    (CACHE, Json(arena_proto::currency::canonical_timezones()))
}

pub(super) fn router() -> Router {
    Router::new()
        .route("/meta/countries", get(countries))
        .route("/meta/timezones", get(timezones))
}
