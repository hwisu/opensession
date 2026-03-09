use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use opensession_api::{
    ParseCandidate as ApiParseCandidate, ParsePreviewErrorResponse, ParsePreviewRequest,
    ParsePreviewResponse,
};
use opensession_parsers::{
    ParseCandidate as ParserParseCandidate, ParseError as ParserParseError, ParserRegistry,
};

use crate::AppConfig;
use crate::storage::Db;

mod auth;
mod errors;
mod fetch;
mod input;
mod remote;

#[cfg(test)]
mod tests;

use auth::resolve_optional_user_id;
use errors::PreviewRouteError;
use input::prepare_parse_input_with_ctx;

const FETCH_TIMEOUT_SECS: u64 = 10;
const MAX_SOURCE_SIZE_BYTES: usize = 10 * 1024 * 1024;

/// POST /api/parse/preview
pub async fn preview(
    State(db): State<Db>,
    State(config): State<AppConfig>,
    headers: HeaderMap,
    Json(req): Json<ParsePreviewRequest>,
) -> Result<Json<ParsePreviewResponse>, (StatusCode, Json<ParsePreviewErrorResponse>)> {
    let user_id =
        resolve_optional_user_id(&headers, &db, &config).map_err(PreviewRouteError::into_http)?;
    let input =
        prepare_parse_input_with_ctx(req.source, Some(&db), Some(&config), user_id.as_deref())
            .await
            .map_err(PreviewRouteError::into_http)?;

    let preview = ParserRegistry::default()
        .preview_bytes(&input.filename, &input.bytes, req.parser_hint.as_deref())
        .map_err(map_parser_error)
        .map_err(PreviewRouteError::into_http)?;

    Ok(Json(ParsePreviewResponse {
        parser_used: preview.parser_used,
        parser_candidates: to_api_candidates(preview.parser_candidates),
        session: preview.session,
        source: input.source,
        warnings: preview.warnings,
        native_adapter: preview.native_adapter,
    }))
}

fn map_parser_error(err: ParserParseError) -> PreviewRouteError {
    match err {
        ParserParseError::InvalidParserHint { hint } => {
            PreviewRouteError::invalid_source(format!("unsupported parser_hint '{hint}'"))
        }
        ParserParseError::ParserSelectionRequired {
            message,
            parser_candidates,
        } => PreviewRouteError::parser_selection_required(message, parser_candidates),
        ParserParseError::ParseFailed {
            message,
            parser_candidates,
        } => PreviewRouteError::parse_failed(message, parser_candidates),
    }
}

pub(super) fn to_api_candidates(candidates: Vec<ParserParseCandidate>) -> Vec<ApiParseCandidate> {
    candidates
        .into_iter()
        .map(|candidate| ApiParseCandidate {
            id: candidate.id,
            confidence: candidate.confidence,
            reason: candidate.reason,
        })
        .collect()
}
