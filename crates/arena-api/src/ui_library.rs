//! The branch game and app library for the Hub (Story 6.3): `GET /ui/library` lists the entries
//! for Settings → Games, and [`link_catalogue`] fills a linked entry's catalogue fields before a
//! `library.upsert` reaches `apply()`. The catalogue cache (`reference.db`) is readable here and
//! nowhere in `apply()`, so this layer is the one that can say what the catalogue calls a game.

use std::sync::Arc;

use arena_core::game_proposals::{CatalogueGame, build_proposals};

use arena_proto::{
    AppError, ErrorCode, LIBRARY_AGE_RATING_MAX, LIBRARY_NAME_MAX, LibraryUpsert, LibraryView,
    ProposalsView, launcher_from_catalogue,
};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, internal, ui_error};

fn cut(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// `GET /ui/library/scans` (same access as `GET /ui/library`): every live paired PC with its last
/// installed-software scan (Story 6.4), ordered by name. A PC that was never scanned has `null`
/// scan fields.
pub(crate) async fn scans(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.acting_session(&token).await {
        return fail(e);
    }
    let reader = Arc::clone(auth.reader());
    match tokio::task::spawn_blocking(move || reader.kiosk_scans()).await {
        Ok(Ok(devices)) => Json(arena_proto::LibraryScansView { devices }).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// `GET /ui/library`: every live entry, enabled or not, ordered by name. For an entry linked to
/// the catalogue the name, launcher and age rating are the cached catalogue's (an entry whose
/// game the cache no longer holds shows what it was saved with).
pub(crate) async fn list(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.acting_session(&token).await {
        return fail(e);
    }
    let reader = Arc::clone(auth.reader());
    let media = auth.media().cloned();
    let read = tokio::task::spawn_blocking(move || -> Result<LibraryView, AppError> {
        let mut entries = reader.library()?;
        if let Some(cache) = media {
            for e in &mut entries {
                let Some(id) = &e.catalog_id else { continue };
                if let Some(game) = cache.game(id)? {
                    e.name = cut(&game.name, LIBRARY_NAME_MAX);
                    e.launcher = launcher_from_catalogue(game.launcher.as_deref()).to_owned();
                    e.age_rating = cut(
                        game.age_rating.as_deref().unwrap_or(""),
                        LIBRARY_AGE_RATING_MAX,
                    );
                }
            }
        }
        Ok(LibraryView { entries })
    })
    .await;
    match read {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

fn catalog_error(message: &str) -> AppError {
    AppError::with_field(ErrorCode::ValidationFailed, "catalog_id", message)
}

/// When `u` names a catalogue game: rewrites its `name`, `launcher` and `age_rating` from the
/// cached catalogue row and refuses an unknown id (`VALIDATION_FAILED` on `catalog_id`). An
/// entry that is already linked to that game keeps the values it is stored with, so editing its
/// category or tags never fails on a catalogue rename; `apply()` guards that they do not change.
pub(crate) async fn link_catalogue(
    auth: &Arc<UiAuth>,
    u: &mut LibraryUpsert,
) -> Result<(), AppError> {
    let Some(catalog_id) = u
        .catalog_id
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
    else {
        return Ok(());
    };
    u.catalog_id = Some(catalog_id.clone());
    if let Some(entry_id) = u.id.clone() {
        let reader = Arc::clone(auth.reader());
        let stored = tokio::task::spawn_blocking(move || reader.library())
            .await
            .map_err(|_| AppError::new(ErrorCode::Internal, "the library read failed"))??
            .into_iter()
            .find(|e| e.id == entry_id);
        if let Some(s) = stored.filter(|s| s.catalog_id.as_deref() == Some(catalog_id.as_str())) {
            u.name = s.name;
            u.launcher = s.launcher;
            u.age_rating = s.age_rating;
            return Ok(());
        }
    }
    let Some(cache) = auth.media().cloned() else {
        return Err(catalog_error(
            "the game catalogue is not available on this hub",
        ));
    };
    let id = catalog_id.clone();
    let game = tokio::task::spawn_blocking(move || cache.game(&id))
        .await
        .map_err(|_| AppError::new(ErrorCode::Internal, "the catalogue read failed"))??
        .ok_or_else(|| catalog_error("this game is not in the catalogue"))?;
    u.name = cut(&game.name, LIBRARY_NAME_MAX);
    u.launcher = launcher_from_catalogue(game.launcher.as_deref()).to_owned();
    u.age_rating = cut(
        game.age_rating.as_deref().unwrap_or(""),
        LIBRARY_AGE_RATING_MAX,
    );
    Ok(())
}

/// `GET /ui/library/proposals` (same access as `GET /ui/library`): "Found on PCs" (Story 6.5).
/// Every executable the PCs' scans found that is not in the library and not dismissed, grouped
/// by catalogue game (or by file name, product name and publisher when unmatched) with the
/// matched name, cover, launcher and category and the PCs it was found on.
pub(crate) async fn proposals(State(auth): State<Arc<UiAuth>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.acting_session(&token).await {
        return fail(e);
    }
    let reader = Arc::clone(auth.reader());
    let media = auth.media().cloned();
    let read = tokio::task::spawn_blocking(move || -> Result<ProposalsView, AppError> {
        let inputs = reader.proposal_inputs()?;
        // A catalogue read that fails shows the game by its file name rather than hiding it.
        let lookup = |id: &str| -> Option<CatalogueGame> {
            let cache = media.as_ref()?;
            let game = cache.game(id).ok().flatten()?;
            Some(CatalogueGame {
                cover_sha256: cache.cover_sha256(id).ok().flatten(),
                name: game.name,
                launcher: game.launcher,
                category: game.category,
                age_rating: game.age_rating,
            })
        };
        Ok(build_proposals(&inputs, &lookup))
    })
    .await;
    match read {
        Ok(Ok(view)) => Json(view).into_response(),
        Ok(Err(e)) => fail(ui_error(e)),
        Err(_) => fail(internal()),
    }
}

/// The largest cover this endpoint sends.
const COVER_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// `GET /ui/library/cover/{sha256}`: a cached catalogue cover for the Hub UI (Story 6.5). Only
/// images the cache holds are sent; anything else is 404.
pub(crate) async fn cover(
    State(auth): State<Arc<UiAuth>>,
    headers: HeaderMap,
    Path(sha256): Path<String>,
) -> Response {
    let Some(token) = bearer(&headers).map(str::to_owned) else {
        return fail(auth_failed());
    };
    if let Err(e) = auth.acting_session(&token).await {
        return fail(e);
    }
    let not_found = || StatusCode::NOT_FOUND.into_response();
    let Some(cache) = auth.media().cloned() else {
        return not_found();
    };
    let now = auth.clock().now_ms();
    let found = tokio::task::spawn_blocking(move || cache.serve_lookup(&sha256, now)).await;
    let file = match found {
        Ok(Ok(Some(f))) if f.content_type.starts_with("image/") && f.bytes <= COVER_MAX_BYTES => f,
        Ok(Ok(_)) => return not_found(),
        Ok(Err(e)) => return fail(ui_error(e)),
        Err(_) => return fail(internal()),
    };
    match tokio::fs::read(&file.path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, file.content_type),
                (header::CACHE_CONTROL, "private, max-age=86400"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => not_found(),
    }
}
