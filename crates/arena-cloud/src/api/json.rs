//! [`ApiJson`]: axum's JSON extractor with every rejection mapped to `BAD_REQUEST`.

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use super::error::ApiError;

/// A JSON request body. A missing JSON content type, invalid JSON or a body that doesn't match
/// `T` is `BAD_REQUEST` (400, naming `field` when serde reports one); a body over the limit is
/// `BAD_REQUEST` with 413. The message never quotes serde (it can name Rust types).
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiJson<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Self(v)),
            Err(e) => Err(rejection(&e)),
        }
    }
}

fn rejection(e: &JsonRejection) -> ApiError {
    match e {
        JsonRejection::JsonDataError(d) => {
            ApiError::bad_request("The body doesn't match the request.", field_of(d))
        }
        JsonRejection::JsonSyntaxError(_) => {
            ApiError::bad_request("The body is not valid JSON.", None)
        }
        JsonRejection::MissingJsonContentType(_) => {
            ApiError::bad_request("Send the body as application/json.", None)
        }
        other if other.status() == StatusCode::PAYLOAD_TOO_LARGE => ApiError::too_large(),
        _ => ApiError::bad_request("The request body could not be read.", None),
    }
}

/// The offending field of a data error: serde's path (`text`, `items[0].name`); for a missing
/// field, the path to the object plus the field serde names (`text`, `items[0].name`).
fn field_of(e: &(dyn std::error::Error + 'static)) -> Option<String> {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = cur {
        if let Some(p) = err.downcast_ref::<serde_path_to_error::Error<serde_json::Error>>() {
            return field_from(&p.path().to_string(), &p.inner().to_string());
        }
        cur = err.source();
    }
    None
}

/// The field for serde path `path` (`.` is the root) and serde message `msg`.
fn field_from(path: &str, msg: &str) -> Option<String> {
    let root = path == ".";
    match missing_field(msg) {
        Some(name) if root => Some(name),
        Some(name) => Some(format!("{path}.{name}")),
        None if root => None,
        None => Some(path.to_owned()),
    }
}

/// `missing field `name`` → `name`.
fn missing_field(msg: &str) -> Option<String> {
    let rest = msg.strip_prefix("missing field `")?;
    let name = rest.split('`').next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_field_names_the_field() {
        assert_eq!(
            missing_field("missing field `text` at line 1 column 2"),
            Some("text".to_owned())
        );
        assert_eq!(missing_field("invalid type: integer `5`"), None);
    }

    #[test]
    fn a_nested_missing_field_is_named_under_its_parent() {
        let missing = "missing field `name` at line 1 column 14";
        assert_eq!(field_from(".", missing), Some("name".to_owned()));
        assert_eq!(
            field_from("items[0]", missing),
            Some("items[0].name".to_owned())
        );
        assert_eq!(
            field_from(
                "items[0].name",
                "invalid type: integer `5`, expected a string"
            ),
            Some("items[0].name".to_owned())
        );
        assert_eq!(field_from(".", "invalid type: map"), None);
    }
}
