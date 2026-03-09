use std::collections::HashSet;

use axum::http::StatusCode;
use base64::Engine;
use opensession_api::{ParsePreviewRequest, ParseSource};
use opensession_parsers::{
    ParseCandidate as ParserParseCandidate, ParseError as ParserParseError, ParserRegistry,
};

use super::MAX_SOURCE_SIZE_BYTES;
use super::errors::PreviewRouteError;
use super::fetch::is_allowed_content_type;
use super::input::{normalize_git_source, normalize_github_source, prepare_parse_input};
use super::map_parser_error;
use super::remote::{build_git_raw_url, path_prefix_matches, provider_for_host};

fn parse_request_for_inline(filename: &str, raw: &[u8]) -> ParsePreviewRequest {
    ParsePreviewRequest {
        source: ParseSource::Inline {
            filename: filename.to_string(),
            content_base64: base64::engine::general_purpose::STANDARD.encode(raw),
        },
        parser_hint: None,
    }
}

#[test]
fn github_source_rejects_invalid_owner() {
    let err = normalize_github_source("bad owner!", "repo", "main", "sessions/a.hail.jsonl")
        .expect_err("owner validation must fail");
    assert_eq!(err.code, "invalid_source");
}

#[test]
fn github_source_rejects_invalid_ref() {
    let err = normalize_github_source("owner", "repo", "main:prod", "sessions/a.hail.jsonl")
        .expect_err("ref validation must fail");
    assert_eq!(err.code, "invalid_source");
}

#[test]
fn github_source_rejects_path_traversal_segments() {
    let err = normalize_github_source("owner", "repo", "main", "sessions/../a.hail.jsonl")
        .expect_err("path traversal must fail");
    assert_eq!(err.code, "invalid_source");
}

#[test]
fn github_source_accepts_normalized_path() {
    let source = normalize_github_source(
        "hwisu",
        "opensession",
        "main",
        "sessions/foo%20bar.hail.jsonl",
    )
    .expect("source should be valid");
    assert_eq!(source.path, "sessions/foo bar.hail.jsonl");
    let remote = reqwest::Url::parse("https://github.com/hwisu/opensession")
        .expect("remote url should parse");
    assert_eq!(
        build_git_raw_url(
            &super::input::normalize_git_source(remote.as_str(), "main", &source.path)
                .expect("normalized"),
            &HashSet::new()
        )
        .expect("github raw url should build"),
        "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/foo%20bar.hail.jsonl"
    );
}

#[test]
fn git_source_rejects_localhost_remote() {
    let err = normalize_git_source(
        "http://localhost:3000/hwisu/opensession",
        "main",
        "sessions/demo.hail.jsonl",
    )
    .expect_err("localhost remote must be rejected");
    assert_eq!(err.code, "invalid_source");
}

#[test]
fn git_source_rejects_private_ip_remote() {
    let err = normalize_git_source(
        "http://192.168.0.10/hwisu/opensession",
        "main",
        "sessions/demo.hail.jsonl",
    )
    .expect_err("private ip remote must be rejected");
    assert_eq!(err.code, "invalid_source");
}

#[test]
fn git_source_rejects_credentials_and_query() {
    let with_credentials = normalize_git_source(
        "https://user:secret@example.com/org/repo",
        "main",
        "sessions/demo.hail.jsonl",
    )
    .expect_err("remote credentials must be rejected");
    assert_eq!(with_credentials.code, "invalid_source");

    let with_query = normalize_git_source(
        "https://example.com/org/repo?token=1",
        "main",
        "sessions/demo.hail.jsonl",
    )
    .expect_err("remote query must be rejected");
    assert_eq!(with_query.code, "invalid_source");
}

#[test]
fn build_git_raw_url_uses_provider_aware_patterns() {
    let no_gitlab_hosts = HashSet::new();
    let mut configured_gitlab_hosts = HashSet::new();
    configured_gitlab_hosts.insert("gitlab.internal.example.com".to_string());

    let github = build_git_raw_url(
        &super::input::normalize_git_source(
            "https://github.com/hwisu/opensession",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect("github source should normalize"),
        &no_gitlab_hosts,
    )
    .expect("github raw url should build");
    assert_eq!(
        github,
        "https://raw.githubusercontent.com/hwisu/opensession/main/sessions/demo.hail.jsonl"
    );

    let gitlab = build_git_raw_url(
        &super::input::normalize_git_source(
            "https://gitlab.com/group/subgroup/repo",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect("gitlab source should normalize"),
        &no_gitlab_hosts,
    )
    .expect("gitlab raw url should build");
    assert_eq!(
        gitlab,
        "https://gitlab.com/group/subgroup/repo/-/raw/main/sessions/demo.hail.jsonl"
    );

    let gitlab_self_managed = build_git_raw_url(
        &super::input::normalize_git_source(
            "https://gitlab.internal.example.com/group/subgroup/repo.git",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect("self-managed gitlab source should normalize"),
        &configured_gitlab_hosts,
    )
    .expect("self-managed gitlab raw url should build");
    assert_eq!(
        gitlab_self_managed,
        "https://gitlab.internal.example.com/group/subgroup/repo/-/raw/main/sessions/demo.hail.jsonl"
    );

    let generic = build_git_raw_url(
        &super::input::normalize_git_source(
            "https://code.example.com/team/repo.git",
            "main",
            "sessions/demo.hail.jsonl",
        )
        .expect("generic source should normalize"),
        &no_gitlab_hosts,
    )
    .expect("generic raw url should build");
    assert_eq!(
        generic,
        "https://code.example.com/team/repo/raw/main/sessions/demo.hail.jsonl"
    );
}

#[tokio::test]
async fn inline_source_too_large_returns_file_too_large() {
    let oversized = vec![b'a'; MAX_SOURCE_SIZE_BYTES + 1];
    let req = parse_request_for_inline("session.hail.jsonl", &oversized);
    let err = prepare_parse_input(req.source)
        .await
        .expect_err("oversized inline source should fail");
    assert_eq!(err.code, "file_too_large");
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn parser_selection_required_maps_to_expected_code() {
    let mapped = map_parser_error(ParserParseError::ParserSelectionRequired {
        message: "select parser".to_string(),
        parser_candidates: vec![ParserParseCandidate {
            id: "codex".to_string(),
            confidence: 91,
            reason: "fixture".to_string(),
        }],
    });

    assert_eq!(mapped.code, "parser_selection_required");
    assert_eq!(mapped.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(mapped.parser_candidates.len(), 1);
    assert_eq!(mapped.parser_candidates[0].id, "codex");
}

#[tokio::test]
async fn parse_failed_maps_to_expected_code() {
    let req = parse_request_for_inline("unknown.txt", b"not jsonl");
    let input = prepare_parse_input(req.source)
        .await
        .expect("inline source should decode");

    let err = ParserRegistry::default()
        .preview_bytes(&input.filename, &input.bytes, None)
        .expect_err("unrecognized source should fail parsing");
    let mapped = map_parser_error(err);

    assert_eq!(mapped.code, "parse_failed");
    assert_eq!(mapped.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn invalid_parser_hint_maps_to_invalid_source() {
    let mapped = map_parser_error(ParserParseError::InvalidParserHint {
        hint: "nope".to_string(),
    });
    assert_eq!(mapped.code, "invalid_source");
    assert_eq!(mapped.status, StatusCode::BAD_REQUEST);
}

#[test]
fn content_type_validation_accepts_text_and_json() {
    assert!(is_allowed_content_type("text/plain; charset=utf-8"));
    assert!(is_allowed_content_type("application/json"));
    assert!(is_allowed_content_type("application/x-ndjson"));
    assert!(!is_allowed_content_type("application/octet-stream"));
}

#[test]
fn provider_detection_and_path_prefix_matching_are_stable() {
    let no_gitlab_hosts = HashSet::new();
    let mut configured_gitlab_hosts = HashSet::new();
    configured_gitlab_hosts.insert("gitlab.internal.example.com".to_string());

    assert_eq!(
        provider_for_host("github.com", &no_gitlab_hosts),
        Some("github")
    );
    assert_eq!(
        provider_for_host("gitlab.com", &no_gitlab_hosts),
        Some("gitlab")
    );
    assert_eq!(
        provider_for_host("gitlab.internal.example.com", &no_gitlab_hosts),
        None
    );
    assert_eq!(
        provider_for_host("gitlab.internal.example.com", &configured_gitlab_hosts),
        Some("gitlab")
    );
    assert_eq!(
        provider_for_host("evil-gitlab.example", &no_gitlab_hosts),
        None
    );
    assert_eq!(
        provider_for_host("code.example.com", &no_gitlab_hosts),
        None
    );

    assert!(path_prefix_matches("group/sub/repo", ""));
    assert!(path_prefix_matches("group/sub/repo", "group/sub"));
    assert!(path_prefix_matches("group/sub/repo", "group/sub/repo"));
    assert!(!path_prefix_matches("group/sub/repo", "group/su"));
    assert!(!path_prefix_matches("group/sub/repo", "group/sub/repo2"));
}

#[test]
fn git_source_rejects_http_remote() {
    let err = normalize_git_source(
        "http://code.example.com/team/repo",
        "main",
        "sessions/demo.hail.jsonl",
    )
    .expect_err("http remote must be rejected");
    assert_eq!(err.code, "invalid_source");
}

#[test]
fn git_credential_error_codes_are_stable() {
    let missing = PreviewRouteError::missing_git_credential(401);
    assert_eq!(missing.code, "missing_git_credential");
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);

    let forbidden = PreviewRouteError::git_credential_forbidden(403);
    assert_eq!(forbidden.code, "git_credential_forbidden");
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
}
