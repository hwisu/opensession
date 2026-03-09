mod collect;
mod parse;
mod types;

pub use collect::{GitCommandRunner, GitSummaryService, ShellGitCommandRunner};
pub use parse::{parse_git_name_status, parse_git_numstat, parse_git_untracked_paths};
pub use types::GitSummaryContext;

#[cfg(test)]
mod tests;
