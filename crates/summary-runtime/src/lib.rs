mod provider;

pub use provider::{
    LocalSummaryProfile, detect_local_summary_profile, generate_summary, generate_text,
};

use opensession_core::trace::Session;
use opensession_runtime_config::SummarySettings;
use opensession_summary::git::{GitSummaryContext, GitSummaryService, ShellGitCommandRunner};
use opensession_summary::{
    GitSummaryRequest, SemanticSummaryArtifact, classify_and_summarize_git_context,
    summarize_session_with_provider,
};
use std::path::Path;

fn runtime_generate_summary<'a>(
    settings: &'a SummarySettings,
    prompt: &'a str,
) -> opensession_summary::SummaryGenerateFuture<'a> {
    Box::pin(provider::generate_summary(settings, prompt))
}

pub async fn summarize_session(
    session: &Session,
    settings: &SummarySettings,
    git_request: Option<&GitSummaryRequest>,
) -> Result<SemanticSummaryArtifact, String> {
    let git_context = if settings.allows_git_changes_fallback() {
        git_request.and_then(collect_git_context)
    } else {
        None
    };

    summarize_session_with_provider(session, settings, git_context, runtime_generate_summary).await
}

pub async fn summarize_git_commit(
    repo_root: &Path,
    commit: &str,
    settings: &SummarySettings,
) -> Result<SemanticSummaryArtifact, String> {
    let request = GitSummaryRequest::from_commit(repo_root.to_path_buf(), commit.to_string());
    let context = collect_git_context(&request)
        .ok_or_else(|| format!("unable to collect git summary context for commit `{commit}`"))?;
    classify_and_summarize_git_context(context, settings, runtime_generate_summary).await
}

pub async fn summarize_git_working_tree(
    repo_root: &Path,
    settings: &SummarySettings,
) -> Result<SemanticSummaryArtifact, String> {
    let request = GitSummaryRequest::working_tree(repo_root.to_path_buf());
    let context = collect_git_context(&request)
        .ok_or_else(|| "unable to collect git summary context for working tree".to_string())?;
    classify_and_summarize_git_context(context, settings, runtime_generate_summary).await
}

fn collect_git_context(request: &GitSummaryRequest) -> Option<GitSummaryContext> {
    let service = GitSummaryService::new(ShellGitCommandRunner);
    if let Some(commit) = request.commit.as_deref() {
        return service.collect_commit_context(
            &request.repo_root,
            commit,
            opensession_summary::MAX_FILE_CHANGE_ENTRIES,
            opensession_summary::classify_arch_layer,
        );
    }

    service.collect_working_tree_context(
        &request.repo_root,
        opensession_summary::MAX_FILE_CHANGE_ENTRIES,
        opensession_summary::classify_arch_layer,
    )
}
