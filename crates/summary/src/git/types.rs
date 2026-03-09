use crate::types::HailCompactFileChange;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct GitSummaryContext {
    pub source: String,
    pub repo_root: PathBuf,
    pub commit: Option<String>,
    pub timeline_signals: Vec<String>,
    pub file_changes: Vec<HailCompactFileChange>,
}
