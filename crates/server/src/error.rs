use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use opensession_api_types::ServiceError;
use std::fmt;

/// Thin Axum adapter around the shared [`ServiceError`] type.
///
/// Keeps the same convenience constructors so route code doesn't change.
/// Produces `{"error": "<message>"}` JSON responses.
pub struct ApiErr(ServiceError);

impl ApiErr {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(ServiceError::BadRequest(msg.into()))
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self(ServiceError::Unauthorized(msg.into()))
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self(ServiceError::Forbidden(msg.into()))
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(ServiceError::NotFound(msg.into()))
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self(ServiceError::Conflict(msg.into()))
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self(ServiceError::Internal(msg.into()))
    }

    /// Build a closure that logs a DB/IO error and returns `500 Internal Server Error`.
    pub fn from_db<E: fmt::Display>(context: &str) -> impl FnOnce(E) -> Self + '_ {
        move |e| {
            tracing::error!("{context}: {e}");
            Self::internal("internal server error")
        }
    }
}

impl From<ServiceError> for ApiErr {
    fn from(e: ServiceError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(opensession_api_types::ApiError::from(&self.0))).into_response()
    }
}
