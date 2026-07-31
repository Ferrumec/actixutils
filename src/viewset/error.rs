use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde_json::json;
use thiserror::Error;

/// Errors that can surface from any layer. Repository errors get wrapped
/// as they bubble up so the ViewSet has one place to translate to HTTP.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("resource not found")]
    NotFound,

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("permission denied")]
    Forbidden,

    #[error("unauthorized")]
    Unauthorized,

    #[error("conflict: {0}")]
    Conflict(String),

    /// Optimistic-lock mismatch (e.g. a version/`updated_at` column check
    /// failed on `UPDATE`).
    ///
    /// Design decision: no default `Repository`/`Service` method produces
    /// this variant — none of them implement optimistic locking. It's
    /// declared here (with its 409 status wired up below) so a developer
    /// who overrides `Repository::update`/`update_in_tx` to add a
    /// version check has a ready-made error to return, instead of
    /// inventing their own `ApiError` variant or reusing `Conflict` and
    /// losing the more specific status/semantics.
    #[error("optimistic lock mismatch")]
    StaleVersion,

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Conflict(_) | ApiError::StaleVersion => StatusCode::CONFLICT,
            ApiError::Database(sqlx::Error::RowNotFound) => StatusCode::NOT_FOUND,
            ApiError::Database(_) | ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();

        // 5xx bodies never carry the raw error text to the client — a
        // `sqlx::Error`/`Internal` message can contain table/column/
        // constraint names or other internals. Log the real error
        // server-side (where it's actually actionable) and return a
        // generic message instead.
        if status.is_server_error() {
            tracing::error!(error = %self, status = %status, "unhandled viewset error");
            return HttpResponse::build(status).json(json!({
                "error": "internal server error",
            }));
        }

        HttpResponse::build(status).json(json!({
            "error": self.to_string(),
        }))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
