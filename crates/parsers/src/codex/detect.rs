use std::path::Path;

pub(super) fn can_parse_codex_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "jsonl")
        && path
            .to_str()
            .is_some_and(|s| s.contains(".codex/sessions") || s.contains("codex/sessions"))
}
