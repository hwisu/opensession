use opensession_api::ServiceError;
use worker::Response;

/// Extension trait: convert a shared [`ServiceError`] into a Worker error response.
pub(crate) trait IntoErrResponse {
    fn into_err_response(self) -> worker::Result<Response>;
}

impl IntoErrResponse for ServiceError {
    fn into_err_response(self) -> worker::Result<Response> {
        match Response::from_json(&opensession_api::ApiError::from(&self)) {
            Ok(resp) => Ok(resp.with_status(self.status_code())),
            Err(_) => Response::error(self.message(), self.status_code()),
        }
    }
}
