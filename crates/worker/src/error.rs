use opensession_api::ServiceError;
use worker::Response;

/// Extension trait: convert a shared [`ServiceError`] into a Worker error response.
pub(crate) trait IntoErrResponse {
    fn into_err_response(self) -> worker::Result<Response>;
}

impl IntoErrResponse for ServiceError {
    fn into_err_response(self) -> worker::Result<Response> {
        Response::error(
            serde_json::to_string(&opensession_api::ApiError::from(&self)).unwrap_or_default(),
            self.status_code(),
        )
    }
}
