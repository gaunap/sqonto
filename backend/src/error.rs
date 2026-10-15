use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// A validation / business rule violation that should be shown to the user (HTTP 400).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct UserError(pub String);

/// Entity not found (HTTP 404).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NotFound(pub String);

/// Error type returned by all HTTP handlers.
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: msg.into() }
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        let err: anyhow::Error = err.into();
        if let Some(u) = err.downcast_ref::<UserError>() {
            return Self::bad(u.0.clone());
        }
        if let Some(n) = err.downcast_ref::<NotFound>() {
            return Self::not_found(n.0.clone());
        }
        if let Some(rusqlite::Error::QueryReturnedNoRows) = err.downcast_ref::<rusqlite::Error>() {
            return Self::not_found("Datensatz nicht gefunden");
        }
        tracing::error!("{err:#}");
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: format!("{err:#}") }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

pub type ApiResult<T> = Result<Json<T>, AppError>;

/// Return early with a user facing error.
#[macro_export]
macro_rules! bail_user {
    ($($arg:tt)*) => {
        return Err($crate::error::UserError(format!($($arg)*)).into())
    };
}
