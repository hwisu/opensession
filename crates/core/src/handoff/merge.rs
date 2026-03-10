use std::collections::{HashMap, HashSet};

use super::{FileChange, HandoffSummary, MergedHandoff};

/// Merge multiple session summaries into a single handoff context.
pub fn merge_summaries(summaries: &[HandoffSummary]) -> MergedHandoff {
    let session_ids: Vec<String> = summaries
        .iter()
        .map(|summary| summary.source_session_id.clone())
        .collect();
    let total_duration: u64 = summaries
        .iter()
        .map(|summary| summary.duration_seconds)
        .sum();
    let total_errors: Vec<String> = summaries
        .iter()
        .flat_map(|summary| {
            summary
                .errors
                .iter()
                .map(move |err| format!("[{}] {}", summary.source_session_id, err))
        })
        .collect();

    let all_modified: HashMap<String, &str> = summaries
        .iter()
        .flat_map(|summary| &summary.files_modified)
        .fold(HashMap::new(), |mut map, file_change| {
            map.entry(file_change.path.clone())
                .or_insert(file_change.action);
            map
        });

    let mut sorted_read: Vec<String> = summaries
        .iter()
        .flat_map(|summary| &summary.files_read)
        .filter(|path| !all_modified.contains_key(path.as_str()))
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sorted_read.sort();

    let mut sorted_modified: Vec<FileChange> = all_modified
        .into_iter()
        .map(|(path, action)| FileChange { path, action })
        .collect();
    sorted_modified.sort_by(|left, right| left.path.cmp(&right.path));

    MergedHandoff {
        source_session_ids: session_ids,
        summaries: summaries.to_vec(),
        all_files_modified: sorted_modified,
        all_files_read: sorted_read,
        total_duration_seconds: total_duration,
        total_errors,
    }
}
