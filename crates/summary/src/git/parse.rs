use crate::types::HailCompactFileChange;
use std::collections::HashMap;

pub fn parse_git_name_status(raw: &str) -> HashMap<String, String> {
    let mut operations = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts = trimmed.split('\t').collect::<Vec<_>>();
        if parts.is_empty() {
            continue;
        }

        let status = parts[0].trim();
        let path = if status.starts_with('R') || status.starts_with('C') {
            parts.get(2).or_else(|| parts.get(1))
        } else {
            parts.get(1)
        };
        let Some(path) = path
            .copied()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let operation = match status.chars().next().unwrap_or('M') {
            'A' => "create",
            'D' => "delete",
            _ => "edit",
        };
        operations.insert(path.to_string(), operation.to_string());
    }
    operations
}

pub fn parse_git_numstat(raw: &str) -> HashMap<String, (u64, u64)> {
    let mut stats = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts = trimmed.split('\t').collect::<Vec<_>>();
        if parts.len() < 3 {
            continue;
        }
        let path = parts[2].trim();
        if path.is_empty() {
            continue;
        }
        let added = parts[0].trim().parse::<u64>().unwrap_or(0);
        let removed = parts[1].trim().parse::<u64>().unwrap_or(0);
        stats.insert(path.to_string(), (added, removed));
    }
    stats
}

pub fn parse_git_untracked_paths(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn build_git_file_changes(
    operation_by_path: HashMap<String, String>,
    numstat_by_path: HashMap<String, (u64, u64)>,
    untracked_paths: Vec<String>,
    max_entries: usize,
    classify_arch_layer: fn(&str) -> &'static str,
) -> Vec<HailCompactFileChange> {
    let mut by_path: HashMap<String, HailCompactFileChange> = HashMap::new();

    for (path, operation) in operation_by_path {
        by_path
            .entry(path.clone())
            .and_modify(|entry| {
                entry.operation = operation.clone();
                entry.layer = classify_arch_layer(&path).to_string();
            })
            .or_insert_with(|| HailCompactFileChange {
                path: path.clone(),
                layer: classify_arch_layer(&path).to_string(),
                operation,
                lines_added: 0,
                lines_removed: 0,
            });
    }

    for (path, (added, removed)) in numstat_by_path {
        let entry = by_path
            .entry(path.clone())
            .or_insert_with(|| HailCompactFileChange {
                path: path.clone(),
                layer: classify_arch_layer(&path).to_string(),
                operation: "edit".to_string(),
                lines_added: 0,
                lines_removed: 0,
            });
        entry.lines_added = entry.lines_added.saturating_add(added);
        entry.lines_removed = entry.lines_removed.saturating_add(removed);
    }

    for path in untracked_paths {
        by_path
            .entry(path.clone())
            .and_modify(|entry| {
                entry.operation = "create".to_string();
                entry.layer = classify_arch_layer(&path).to_string();
            })
            .or_insert_with(|| HailCompactFileChange {
                path: path.clone(),
                layer: classify_arch_layer(&path).to_string(),
                operation: "create".to_string(),
                lines_added: 0,
                lines_removed: 0,
            });
    }

    let mut changes = by_path.into_values().collect::<Vec<_>>();
    changes.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path));
    changes.truncate(max_entries);
    changes
}

pub(crate) fn diff_preview_lines(raw: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut iter = raw.lines();
    for _ in 0..max_lines {
        let Some(line) = iter.next() else {
            break;
        };
        lines.push(truncate_preview_line(line, max_chars));
    }
    if iter.next().is_some() {
        lines.push("…".to_string());
    }
    lines
}

fn truncate_preview_line(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        return raw.to_string();
    }
    let mut out = String::new();
    for ch in raw.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}
