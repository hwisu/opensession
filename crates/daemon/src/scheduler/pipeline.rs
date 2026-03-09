use anyhow::Result;
use chrono::{DateTime, Utc};
use opensession_core::Session;
use opensession_core::session::{GitMeta, interaction_compressed_session, is_auxiliary_session};
use opensession_git_native::{
    SUMMARY_LEDGER_REF, SessionSummaryLedgerRecord, branch_ledger_ref, extract_git_context,
    resolve_ledger_branch,
};
use opensession_local_db::LocalDb;
use opensession_parsers::ParserRegistry;
use opensession_runtime_config::SummaryStorageBackend;
use opensession_summary::{GitSummaryRequest, summarize_session};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::config::{DaemonConfig, GitStorageMethod, SessionDefaultView};
use crate::repo_registry::RepoRegistry;

use super::config_resolution::resolve_effective_config;
use super::git_retention::collect_commit_shas_for_session;
use super::helpers::{
    build_session_meta_json, enum_label, sanitize, session_cwd, session_to_hail_jsonl_bytes,
};

pub(super) async fn process_file(
    path: &PathBuf,
    config: &DaemonConfig,
    db: &LocalDb,
    repo_registry: &mut RepoRegistry,
    auto_upload: bool,
) -> Result<()> {
    if was_already_uploaded(path, db)? {
        return Ok(());
    }

    let mut session = match parse_session(path)? {
        Some(session) => session,
        None => return Ok(()),
    };

    let effective_config = resolve_effective_config(&session, config);

    if is_tool_excluded(&session, &effective_config) {
        return Ok(());
    }

    store_locally(&session, path, db, &effective_config)?;
    if let Err(error) = maybe_generate_semantic_summary(&session, db, &effective_config).await {
        warn!(
            session_id = %session.session_id,
            "semantic summary generation skipped/failed: {error}"
        );
    }

    if !auto_upload {
        return Ok(());
    }

    sanitize(&mut session, &effective_config);

    let git_store = maybe_git_store(&session, &effective_config);
    if let Some(ref stored) = git_store {
        if let Err(error) = repo_registry.add(&stored.repo_root) {
            warn!(
                repo = %stored.repo_root.display(),
                "failed to update repo registry: {error}"
            );
        }
    }

    mark_session_share_ready(
        &session,
        db,
        git_store
            .as_ref()
            .and_then(|stored| stored.body_url.as_deref()),
    )
}

pub(super) fn was_already_uploaded(path: &PathBuf, db: &LocalDb) -> Result<bool> {
    let modified: DateTime<Utc> = std::fs::metadata(path)?.modified()?.into();
    let path_str = path.to_string_lossy().to_string();
    if db.was_uploaded_after(&path_str, &modified)? {
        debug!("Skipping already-uploaded file: {}", path.display());
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn parse_session(path: &Path) -> Result<Option<Session>> {
    let session = match ParserRegistry::default().parse_path(path)? {
        Some(session) => session,
        None => {
            warn!("No parser for: {}", path.display());
            return Ok(None);
        }
    };
    if is_auxiliary_session(&session) {
        debug!("Skipping auxiliary session from {}", path.display());
        return Ok(None);
    }

    info!("Parsing: {}", path.display());
    Ok(Some(session))
}

pub(super) fn is_tool_excluded(session: &Session, config: &DaemonConfig) -> bool {
    let excluded = config
        .privacy
        .exclude_tools
        .iter()
        .any(|tool| tool.eq_ignore_ascii_case(&session.agent.tool));

    if excluded {
        info!(
            "Excluding tool '{}': source file excluded by config",
            session.agent.tool,
        );
    }
    excluded
}

pub(super) fn store_locally(
    session: &Session,
    path: &Path,
    db: &LocalDb,
    config: &DaemonConfig,
) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let local_session = if matches!(
        config.daemon.session_default_view,
        SessionDefaultView::Compressed
    ) {
        interaction_compressed_session(session)
    } else {
        session.clone()
    };

    let git = session_cwd(&local_session)
        .map(extract_git_context)
        .unwrap_or_default();
    let local_git = opensession_local_db::git::GitContext {
        remote: git.remote.clone(),
        branch: git.branch.clone(),
        commit: git.commit.clone(),
        repo_name: git.repo_name.clone(),
    };

    db.upsert_local_session(&local_session, &path_str, &local_git)?;
    match std::fs::read(path) {
        Ok(body) => {
            if let Err(error) = db.cache_body(&session.session_id, &body) {
                warn!(
                    "Failed to cache source body for session {}: {}",
                    session.session_id, error
                );
            }
        }
        Err(error) => {
            warn!(
                "Failed to read source file for session {} while caching body: {}",
                session.session_id, error
            );
        }
    }
    Ok(())
}

pub(super) async fn maybe_generate_semantic_summary(
    session: &Session,
    db: &LocalDb,
    config: &DaemonConfig,
) -> Result<()> {
    let settings = &config.summary;
    if !settings.should_generate_on_session_save() {
        return Ok(());
    }
    if settings.storage.backend == SummaryStorageBackend::None {
        return Ok(());
    }
    if !settings.is_configured() {
        return Ok(());
    }

    let git_request = if settings.allows_git_changes_fallback() {
        session_cwd(session).and_then(|cwd| {
            crate::config::find_repo_root(cwd).map(|repo_root| GitSummaryRequest {
                repo_root,
                commit: extract_git_context(cwd).commit,
            })
        })
    } else {
        None
    };

    let artifact = summarize_session(session, settings, git_request.as_ref())
        .await
        .map_err(anyhow::Error::msg)?;

    match settings.storage.backend {
        SummaryStorageBackend::LocalDb => {
            let summary_json = serde_json::to_string(&artifact.summary)?;
            let source_details_json = if artifact.source_details.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&artifact.source_details)?)
            };
            let diff_tree_json = if artifact.diff_tree.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&artifact.diff_tree)?)
            };
            let generated_at = chrono::Utc::now().to_rfc3339();
            let provider = enum_label(&artifact.provider);
            let source_kind = enum_label(&artifact.source_kind);
            let generation_kind = enum_label(&artifact.generation_kind);
            let model = if artifact.model.trim().is_empty() {
                None
            } else {
                Some(artifact.model.clone())
            };
            let prompt_fingerprint = if artifact.prompt_fingerprint.trim().is_empty() {
                None
            } else {
                Some(artifact.prompt_fingerprint)
            };

            db.upsert_session_semantic_summary(
                &opensession_local_db::SessionSemanticSummaryUpsert {
                    session_id: &session.session_id,
                    summary_json: &summary_json,
                    generated_at: &generated_at,
                    provider: &provider,
                    model: model.as_deref(),
                    source_kind: &source_kind,
                    generation_kind: &generation_kind,
                    prompt_fingerprint: prompt_fingerprint.as_deref(),
                    source_details_json: source_details_json.as_deref(),
                    diff_tree_json: diff_tree_json.as_deref(),
                    error: artifact.error.as_deref(),
                },
            )?;
        }
        SummaryStorageBackend::HiddenRef => {
            let cwd = session_cwd(session)
                .ok_or_else(|| anyhow::anyhow!("session working directory is missing"))?;
            let repo_root = crate::config::find_repo_root(cwd)
                .ok_or_else(|| anyhow::anyhow!("failed to resolve git repo root"))?;
            let summary_value = serde_json::to_value(&artifact.summary)?;
            let source_details = serde_json::to_value(&artifact.source_details)?;
            let diff_tree_value = serde_json::to_value(&artifact.diff_tree)?;
            let diff_tree = diff_tree_value.as_array().cloned().unwrap_or_default();
            let record = SessionSummaryLedgerRecord {
                session_id: session.session_id.clone(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                provider: enum_label(&artifact.provider),
                model: (!artifact.model.trim().is_empty()).then_some(artifact.model.clone()),
                source_kind: enum_label(&artifact.source_kind),
                generation_kind: enum_label(&artifact.generation_kind),
                prompt_fingerprint: (!artifact.prompt_fingerprint.trim().is_empty())
                    .then_some(artifact.prompt_fingerprint),
                summary: summary_value,
                source_details,
                diff_tree,
                error: artifact.error.clone(),
            };
            opensession_git_native::NativeGitStorage
                .store_summary_at_ref(&repo_root, SUMMARY_LEDGER_REF, &record)
                .map_err(anyhow::Error::msg)?;
        }
        SummaryStorageBackend::None => {}
    }

    Ok(())
}

pub(super) struct GitStoreOutcome {
    pub(super) body_url: Option<String>,
    pub(super) repo_root: PathBuf,
}

pub(super) fn maybe_git_store(session: &Session, config: &DaemonConfig) -> Option<GitStoreOutcome> {
    if config.git_storage.method == GitStorageMethod::Sqlite {
        return None;
    }

    let cwd = session_cwd(session)?;
    let repo_root = crate::config::find_repo_root(cwd)?;
    let git_ctx = extract_git_context(cwd);
    let branch = resolve_ledger_branch(git_ctx.branch.as_deref(), git_ctx.commit.as_deref());
    let ref_name = branch_ledger_ref(&branch);
    let commit_shas = collect_commit_shas_for_session(&repo_root, session);

    let hail_jsonl = session_to_hail_jsonl_bytes(session)?;
    let git_meta = GitMeta {
        remote: git_ctx.remote.clone(),
        repo_name: git_ctx.repo_name.clone(),
        branch: Some(branch),
        head: git_ctx.commit.clone(),
        commits: commit_shas.clone(),
    };
    let meta_json = build_session_meta_json(session, Some(&git_meta));

    let storage = opensession_git_native::NativeGitStorage;
    match storage.store_session_at_ref(
        &repo_root,
        &ref_name,
        &session.session_id,
        &hail_jsonl,
        &meta_json,
        &commit_shas,
    ) {
        Ok(stored) => {
            info!(
                "Stored session {} to git ref {} at {}",
                session.session_id, stored.ref_name, stored.hail_path
            );
            let body_url = git_ctx.remote.as_ref().map(|remote| {
                opensession_git_native::generate_raw_url(
                    remote,
                    &stored.commit_id,
                    &stored.hail_path,
                )
            });
            Some(GitStoreOutcome {
                body_url,
                repo_root,
            })
        }
        Err(error) => {
            warn!(
                "Git-native store failed for session {}: {}",
                session.session_id, error
            );
            None
        }
    }
}

pub(super) fn mark_session_share_ready(
    session: &Session,
    db: &LocalDb,
    body_url: Option<&str>,
) -> Result<()> {
    if let Some(url) = body_url {
        info!(
            "Session {} stored in git-native ledger and share-ready ({})",
            session.session_id, url
        );
    } else {
        info!(
            "Session {} indexed locally. Share with CLI quick flow: opensession share os://src/local/<sha256> --quick",
            session.session_id
        );
    }
    db.mark_synced(&session.session_id)?;
    Ok(())
}
