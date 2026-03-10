//! Shared API types, crypto, and SQL builders for opensession.io
//!
//! This crate is the **single source of truth** for all API request/response types.
//! TypeScript types are auto-generated via `ts-rs` and consumed by the frontend.
//!
//! To regenerate TypeScript types:
//!   cargo test -p opensession-api -- export_typescript --nocapture

#[cfg(feature = "backend")]
pub mod crypto;
#[cfg(feature = "backend")]
pub mod db;
pub mod deploy;
pub mod oauth;
pub mod parse_preview_source;
#[cfg(feature = "backend")]
pub mod service;

mod auth_types;
mod desktop_runtime_types;
mod errors;
mod local_review_types;
mod parse_preview_types;
mod session_types;
mod shared_types;

pub use auth_types::{
    AuthRegisterRequest, AuthTokenResponse, ChangePasswordRequest, CreateGitCredentialRequest,
    GitCredentialSummary, IssueApiKeyResponse, ListGitCredentialsResponse, LoginRequest,
    LogoutRequest, OAuthLinkResponse, OkResponse, RefreshRequest, UserSettingsResponse,
    VerifyResponse,
};
pub use desktop_runtime_types::{
    DESKTOP_IPC_CONTRACT_VERSION, DesktopChangeQuestionRequest, DesktopChangeQuestionResponse,
    DesktopChangeReadRequest, DesktopChangeReadResponse, DesktopChangeReaderScope,
    DesktopChangeReaderTtsRequest, DesktopChangeReaderTtsResponse,
    DesktopChangeReaderVoiceProvider, DesktopContractVersionResponse, DesktopHandoffBuildRequest,
    DesktopHandoffBuildResponse, DesktopLifecycleCleanupState,
    DesktopLifecycleCleanupStatusResponse, DesktopQuickShareRequest, DesktopQuickShareResponse,
    DesktopRuntimeChangeReaderSettings, DesktopRuntimeChangeReaderSettingsUpdate,
    DesktopRuntimeChangeReaderVoiceSettings, DesktopRuntimeChangeReaderVoiceSettingsUpdate,
    DesktopRuntimeLifecycleSettings, DesktopRuntimeLifecycleSettingsUpdate,
    DesktopRuntimeSettingsResponse, DesktopRuntimeSettingsUpdateRequest,
    DesktopRuntimeSummaryBatchSettings, DesktopRuntimeSummaryBatchSettingsUpdate,
    DesktopRuntimeSummaryPromptSettings, DesktopRuntimeSummaryPromptSettingsUpdate,
    DesktopRuntimeSummaryProviderSettings, DesktopRuntimeSummaryProviderSettingsUpdate,
    DesktopRuntimeSummaryResponseSettings, DesktopRuntimeSummaryResponseSettingsUpdate,
    DesktopRuntimeSummarySettings, DesktopRuntimeSummarySettingsUpdate,
    DesktopRuntimeSummaryStorageSettings, DesktopRuntimeSummaryStorageSettingsUpdate,
    DesktopRuntimeSummaryUiConstraints, DesktopRuntimeVectorSearchSettings,
    DesktopRuntimeVectorSearchSettingsUpdate, DesktopSessionSummaryResponse,
    DesktopSummaryBatchExecutionMode, DesktopSummaryBatchScope, DesktopSummaryBatchState,
    DesktopSummaryBatchStatusResponse, DesktopSummaryOutputShape,
    DesktopSummaryProviderDetectResponse, DesktopSummaryProviderId,
    DesktopSummaryProviderTransport, DesktopSummaryResponseStyle, DesktopSummarySourceMode,
    DesktopSummaryStorageBackend, DesktopSummaryTriggerMode, DesktopVectorChunkingMode,
    DesktopVectorIndexState, DesktopVectorIndexStatusResponse, DesktopVectorInstallState,
    DesktopVectorInstallStatusResponse, DesktopVectorPreflightResponse,
    DesktopVectorSearchGranularity, DesktopVectorSearchProvider, DesktopVectorSearchResponse,
    DesktopVectorSessionMatch,
};
pub use errors::{ApiError, DesktopApiError, ServiceError};
pub use local_review_types::{
    LocalReviewBundle, LocalReviewCommit, LocalReviewLayerFileChange, LocalReviewPrMeta,
    LocalReviewReviewerDigest, LocalReviewReviewerQa, LocalReviewSemanticSummary,
    LocalReviewSession,
};
pub use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext, Stats,
};
pub use parse_preview_types::{
    ParseCandidate, ParsePreviewErrorResponse, ParsePreviewRequest, ParsePreviewResponse,
    ParseSource,
};
pub use session_types::{
    CapabilitiesResponse, DEFAULT_REGISTER_TARGETS, DEFAULT_SHARE_MODES, DesktopSessionListQuery,
    HealthResponse, SessionDetail, SessionLink, SessionListQuery, SessionListResponse,
    SessionRepoListResponse, SessionSummary, StreamEventsRequest, StreamEventsResponse,
    UploadRequest, UploadResponse,
};
pub use shared_types::{LinkType, SortOrder, TimeRange, saturating_i64};

#[cfg(test)]
mod schema_tests {
    use super::*;

    #[test]
    fn parse_preview_request_round_trip_git() {
        let req = ParsePreviewRequest {
            source: ParseSource::Git {
                remote: "https://github.com/hwisu/opensession".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            parser_hint: Some("hail".to_string()),
        };

        let json = serde_json::to_string(&req).expect("request should serialize");
        let decoded: ParsePreviewRequest =
            serde_json::from_str(&json).expect("request should deserialize");

        match decoded.source {
            ParseSource::Git {
                remote,
                r#ref,
                path,
            } => {
                assert_eq!(remote, "https://github.com/hwisu/opensession");
                assert_eq!(r#ref, "main");
                assert_eq!(path, "sessions/demo.hail.jsonl");
            }
            _ => panic!("expected git parse source"),
        }
        assert_eq!(decoded.parser_hint.as_deref(), Some("hail"));
    }

    #[test]
    fn parse_preview_request_round_trip_github_compat() {
        let req = ParsePreviewRequest {
            source: ParseSource::Github {
                owner: "hwisu".to_string(),
                repo: "opensession".to_string(),
                r#ref: "main".to_string(),
                path: "sessions/demo.hail.jsonl".to_string(),
            },
            parser_hint: Some("hail".to_string()),
        };

        let json = serde_json::to_string(&req).expect("request should serialize");
        let decoded: ParsePreviewRequest =
            serde_json::from_str(&json).expect("request should deserialize");

        match decoded.source {
            ParseSource::Github {
                owner,
                repo,
                r#ref,
                path,
            } => {
                assert_eq!(owner, "hwisu");
                assert_eq!(repo, "opensession");
                assert_eq!(r#ref, "main");
                assert_eq!(path, "sessions/demo.hail.jsonl");
            }
            _ => panic!("expected github parse source"),
        }
        assert_eq!(decoded.parser_hint.as_deref(), Some("hail"));
    }

    #[test]
    fn parse_preview_error_response_round_trip_with_candidates() {
        let payload = ParsePreviewErrorResponse {
            code: "parser_selection_required".to_string(),
            message: "choose parser".to_string(),
            parser_candidates: vec![ParseCandidate {
                id: "codex".to_string(),
                confidence: 89,
                reason: "event markers".to_string(),
            }],
        };

        let json = serde_json::to_string(&payload).expect("error payload should serialize");
        let decoded: ParsePreviewErrorResponse =
            serde_json::from_str(&json).expect("error payload should deserialize");

        assert_eq!(decoded.code, "parser_selection_required");
        assert_eq!(decoded.parser_candidates.len(), 1);
        assert_eq!(decoded.parser_candidates[0].id, "codex");
    }

    #[test]
    fn local_review_bundle_round_trip() {
        let mut sample_session = Session::new(
            "s-review-1".to_string(),
            Agent {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
                tool: "codex".to_string(),
                tool_version: None,
            },
        );
        sample_session.recompute_stats();

        let payload = LocalReviewBundle {
            review_id: "gh-org-repo-pr1-abc1234".to_string(),
            generated_at: "2026-02-24T00:00:00Z".to_string(),
            pr: LocalReviewPrMeta {
                url: "https://github.com/org/repo/pull/1".to_string(),
                owner: "org".to_string(),
                repo: "repo".to_string(),
                number: 1,
                remote: "origin".to_string(),
                base_sha: "a".repeat(40),
                head_sha: "b".repeat(40),
            },
            commits: vec![LocalReviewCommit {
                sha: "c".repeat(40),
                title: "feat: add review flow".to_string(),
                author_name: "Alice".to_string(),
                author_email: "alice@example.com".to_string(),
                authored_at: "2026-02-24T00:00:00Z".to_string(),
                session_ids: vec!["s-review-1".to_string()],
                reviewer_digest: LocalReviewReviewerDigest {
                    qa: vec![LocalReviewReviewerQa {
                        question: "Which route should we verify first?".to_string(),
                        answer: Some("Start with /review/local/:id live path.".to_string()),
                    }],
                    modified_files: vec![
                        "crates/cli/src/review.rs".to_string(),
                        "web/src/routes/review/local/[id]/+page.svelte".to_string(),
                    ],
                    test_files: vec!["web/e2e-live/live-review-local.spec.ts".to_string()],
                },
                semantic_summary: Some(LocalReviewSemanticSummary {
                    changes: "Updated review flow wiring".to_string(),
                    auth_security: "none detected".to_string(),
                    layer_file_changes: vec![LocalReviewLayerFileChange {
                        layer: "application".to_string(),
                        summary: "Added bundle resolver".to_string(),
                        files: vec!["crates/cli/src/review.rs".to_string()],
                    }],
                    source_kind: "git_commit".to_string(),
                    generation_kind: "heuristic_fallback".to_string(),
                    provider: "disabled".to_string(),
                    model: None,
                    error: None,
                    diff_tree: Vec::new(),
                }),
            }],
            sessions: vec![LocalReviewSession {
                session_id: "s-review-1".to_string(),
                ledger_ref: "refs/remotes/origin/opensession/branches/bWFpbg".to_string(),
                hail_path: "v1/se/s-review-1.hail.jsonl".to_string(),
                commit_shas: vec!["c".repeat(40)],
                session: sample_session,
            }],
        };

        let json = serde_json::to_string(&payload).expect("review bundle should serialize");
        let decoded: LocalReviewBundle =
            serde_json::from_str(&json).expect("review bundle should deserialize");

        assert_eq!(decoded.review_id, "gh-org-repo-pr1-abc1234");
        assert_eq!(decoded.pr.number, 1);
        assert_eq!(decoded.commits.len(), 1);
        assert_eq!(decoded.sessions.len(), 1);
        assert_eq!(decoded.sessions[0].session_id, "s-review-1");
        assert_eq!(
            decoded.commits[0]
                .reviewer_digest
                .qa
                .first()
                .map(|row| row.question.as_str()),
            Some("Which route should we verify first?")
        );
        assert_eq!(decoded.commits[0].reviewer_digest.test_files.len(), 1);
    }

    #[test]
    fn capabilities_response_round_trip_includes_new_fields() {
        let caps = CapabilitiesResponse::for_runtime(true, true);

        let json = serde_json::to_string(&caps).expect("capabilities should serialize");
        let decoded: CapabilitiesResponse =
            serde_json::from_str(&json).expect("capabilities should deserialize");

        assert!(decoded.auth_enabled);
        assert!(decoded.parse_preview_enabled);
        assert_eq!(decoded.register_targets, vec!["local", "git"]);
        assert_eq!(decoded.share_modes, vec!["web", "git", "quick", "json"]);
    }

    #[test]
    fn capabilities_defaults_are_stable() {
        assert_eq!(DEFAULT_REGISTER_TARGETS, &["local", "git"]);
        assert_eq!(DEFAULT_SHARE_MODES, &["web", "git", "quick", "json"]);
    }
}

#[cfg(all(test, feature = "ts"))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use ts_rs::TS;

    #[test]
    fn export_typescript() {
        let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/ui/src/api-types.generated.ts");

        let cfg = ts_rs::Config::new().with_large_int("number");
        let mut parts: Vec<String> = Vec::new();
        parts.push("// AUTO-GENERATED by opensession-api — DO NOT EDIT".to_string());
        parts.push(
            "// Regenerate with: cargo test -p opensession-api -- export_typescript".to_string(),
        );
        parts.push(String::new());

        macro_rules! collect_ts {
            ($($t:ty),+ $(,)?) => {
                $(
                    let decl = <$t>::decl(&cfg);
                    let is_struct_decl = decl.contains(" = {") && !decl.contains("} |");
                    let decl = if is_struct_decl {
                        decl
                            .replacen("type ", "export interface ", 1)
                            .replace(" = {", " {")
                            .trim_end_matches(';')
                            .to_string()
                    } else {
                        decl
                            .replacen("type ", "export type ", 1)
                            .trim_end_matches(';')
                            .to_string()
                    };
                    parts.push(decl);
                    parts.push(String::new());
                )+
            };
        }

        collect_ts!(
            SortOrder,
            TimeRange,
            LinkType,
            AuthRegisterRequest,
            LoginRequest,
            AuthTokenResponse,
            RefreshRequest,
            LogoutRequest,
            ChangePasswordRequest,
            VerifyResponse,
            UserSettingsResponse,
            OkResponse,
            IssueApiKeyResponse,
            GitCredentialSummary,
            ListGitCredentialsResponse,
            CreateGitCredentialRequest,
            OAuthLinkResponse,
            UploadResponse,
            SessionSummary,
            SessionListResponse,
            SessionListQuery,
            DesktopSessionListQuery,
            SessionRepoListResponse,
            DesktopHandoffBuildRequest,
            DesktopHandoffBuildResponse,
            DesktopQuickShareRequest,
            DesktopQuickShareResponse,
            DesktopContractVersionResponse,
            DesktopSummaryProviderId,
            DesktopSummaryProviderTransport,
            DesktopSummarySourceMode,
            DesktopSummaryResponseStyle,
            DesktopSummaryOutputShape,
            DesktopSummaryTriggerMode,
            DesktopSummaryStorageBackend,
            DesktopSummaryBatchExecutionMode,
            DesktopSummaryBatchScope,
            DesktopRuntimeSummaryProviderSettings,
            DesktopRuntimeSummaryPromptSettings,
            DesktopRuntimeSummaryResponseSettings,
            DesktopRuntimeSummaryStorageSettings,
            DesktopRuntimeSummaryBatchSettings,
            DesktopRuntimeSummarySettings,
            DesktopRuntimeSummaryProviderSettingsUpdate,
            DesktopRuntimeSummaryPromptSettingsUpdate,
            DesktopRuntimeSummaryResponseSettingsUpdate,
            DesktopRuntimeSummaryStorageSettingsUpdate,
            DesktopRuntimeSummaryBatchSettingsUpdate,
            DesktopRuntimeSummarySettingsUpdate,
            DesktopRuntimeSummaryUiConstraints,
            DesktopVectorSearchProvider,
            DesktopVectorSearchGranularity,
            DesktopVectorChunkingMode,
            DesktopVectorInstallState,
            DesktopVectorIndexState,
            DesktopRuntimeVectorSearchSettings,
            DesktopRuntimeVectorSearchSettingsUpdate,
            DesktopChangeReaderScope,
            DesktopChangeReaderVoiceProvider,
            DesktopRuntimeChangeReaderVoiceSettings,
            DesktopRuntimeChangeReaderVoiceSettingsUpdate,
            DesktopRuntimeChangeReaderSettings,
            DesktopRuntimeChangeReaderSettingsUpdate,
            DesktopRuntimeLifecycleSettings,
            DesktopRuntimeLifecycleSettingsUpdate,
            DesktopLifecycleCleanupState,
            DesktopLifecycleCleanupStatusResponse,
            DesktopVectorPreflightResponse,
            DesktopVectorInstallStatusResponse,
            DesktopVectorIndexStatusResponse,
            DesktopSummaryBatchState,
            DesktopSummaryBatchStatusResponse,
            DesktopVectorSessionMatch,
            DesktopVectorSearchResponse,
            DesktopRuntimeSettingsResponse,
            DesktopRuntimeSettingsUpdateRequest,
            DesktopSummaryProviderDetectResponse,
            DesktopSessionSummaryResponse,
            DesktopChangeReadRequest,
            DesktopChangeReadResponse,
            DesktopChangeQuestionRequest,
            DesktopChangeReaderTtsRequest,
            DesktopChangeReaderTtsResponse,
            DesktopChangeQuestionResponse,
            DesktopApiError,
            SessionDetail,
            SessionLink,
            ParseSource,
            ParseCandidate,
            ParsePreviewRequest,
            ParsePreviewResponse,
            ParsePreviewErrorResponse,
            LocalReviewBundle,
            LocalReviewPrMeta,
            LocalReviewReviewerQa,
            LocalReviewReviewerDigest,
            LocalReviewCommit,
            LocalReviewLayerFileChange,
            LocalReviewSemanticSummary,
            LocalReviewSession,
            oauth::AuthProvidersResponse,
            oauth::OAuthProviderInfo,
            oauth::LinkedProvider,
            HealthResponse,
            CapabilitiesResponse,
            ApiError,
        );

        let content = parts.join("\n");

        if let Some(parent) = out_dir.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut file = std::fs::File::create(&out_dir)
            .unwrap_or_else(|e| panic!("Failed to create {}: {}", out_dir.display(), e));
        file.write_all(content.as_bytes())
            .unwrap_or_else(|e| panic!("Failed to write {}: {}", out_dir.display(), e));

        println!("Generated TypeScript types at: {}", out_dir.display());
    }
}
