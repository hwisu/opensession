use axum::{Json, http::StatusCode};
use opensession_api::{ParseCandidate as ApiParseCandidate, ParsePreviewErrorResponse};
use opensession_parsers::ParseCandidate as ParserParseCandidate;

use super::{MAX_SOURCE_SIZE_BYTES, to_api_candidates};

#[derive(Debug, Clone)]
pub(super) struct PreviewRouteError {
    pub(super) status: StatusCode,
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) parser_candidates: Vec<ApiParseCandidate>,
}

impl PreviewRouteError {
    pub(super) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    pub(super) fn invalid_source(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_source",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    pub(super) fn fetch_failed(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "fetch_failed",
            message: message.into(),
            parser_candidates: Vec::new(),
        }
    }

    pub(super) fn missing_git_credential(status_code: u16) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "missing_git_credential",
            message: format!(
                "remote source returned status {status_code}; connect provider OAuth or register a git credential for this host"
            ),
            parser_candidates: Vec::new(),
        }
    }

    pub(super) fn git_credential_forbidden(status_code: u16) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "git_credential_forbidden",
            message: format!(
                "configured credential was rejected by remote source (status {status_code})"
            ),
            parser_candidates: Vec::new(),
        }
    }

    pub(super) fn file_too_large(size: usize) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "file_too_large",
            message: format!(
                "source is too large ({} bytes, max {} bytes)",
                size, MAX_SOURCE_SIZE_BYTES
            ),
            parser_candidates: Vec::new(),
        }
    }

    pub(super) fn parse_failed(
        message: impl Into<String>,
        parser_candidates: Vec<ParserParseCandidate>,
    ) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "parse_failed",
            message: message.into(),
            parser_candidates: to_api_candidates(parser_candidates),
        }
    }

    pub(super) fn parser_selection_required(
        message: impl Into<String>,
        parser_candidates: Vec<ParserParseCandidate>,
    ) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "parser_selection_required",
            message: message.into(),
            parser_candidates: to_api_candidates(parser_candidates),
        }
    }

    pub(super) fn into_http(self) -> (StatusCode, Json<ParsePreviewErrorResponse>) {
        (
            self.status,
            Json(ParsePreviewErrorResponse {
                code: self.code.to_string(),
                message: self.message,
                parser_candidates: self.parser_candidates,
            }),
        )
    }
}
