//! The branch game and app library for the Hub (Story 6.3): `GET /ui/library` lists the entries
//! for Settings → Games, and [`link_catalogue`] fills a linked entry's catalogue fields before a
//! `library.upsert` reaches `apply()`. The catalogue cache (`reference.db`) is readable here and
//! nowhere in `apply()`, so this layer is the one that can say what the catalogue calls a game.

use std::sync::Arc;

use arena_proto::{
    AppError, ErrorCode, LIBRARY_AGE_RATING_MAX, LIBRARY_NAME_MAX, LibraryUpsert, LibraryView,
    launcher_from_catalogue,
};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::ui_auth::{UiAuth, auth_failed, bearer, fail, internal, ui_error};

fn cut(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
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
