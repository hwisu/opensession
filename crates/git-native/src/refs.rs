use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use crate::BRANCH_LEDGER_REF_PREFIX;

/// Encode a branch name as base64url (without padding) for ref-safe storage.
pub fn encode_branch_component(branch: &str) -> String {
    URL_SAFE_NO_PAD.encode(branch.as_bytes())
}

/// Return hidden ref name for branch ledger storage.
pub fn branch_ledger_ref(branch: &str) -> String {
    let encoded = encode_branch_component(branch);
    format!("{BRANCH_LEDGER_REF_PREFIX}/{encoded}")
}

/// Normalize branch identity for ledger storage.
///
/// Rules:
/// - Normal branch names are used as-is.
/// - Detached HEAD (`HEAD`/empty branch) maps to `detached@<head8>` when HEAD exists.
/// - Falls back to `detached` if no usable HEAD hash is available.
pub fn resolve_ledger_branch(branch: Option<&str>, head: Option<&str>) -> String {
    let branch = branch.unwrap_or("").trim();
    if !branch.is_empty() && !branch.eq_ignore_ascii_case("HEAD") {
        return branch.to_string();
    }

    let head = head.unwrap_or("").trim();
    let hex: String = head.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() >= 8 {
        return format!("detached@{}", &hex[..8]);
    }

    "detached".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_branch_component_is_deterministic() {
        let encoded = encode_branch_component("feature/hello-world");
        assert_eq!(encoded, "ZmVhdHVyZS9oZWxsby13b3JsZA");
    }

    #[test]
    fn branch_ledger_ref_builds_hidden_ref() {
        let ref_name = branch_ledger_ref("main");
        assert_eq!(ref_name, "refs/opensession/branches/bWFpbg");
    }

    #[test]
    fn resolve_ledger_branch_keeps_named_branch() {
        let branch = resolve_ledger_branch(Some("feature/x"), Some("abcd1234"));
        assert_eq!(branch, "feature/x");
    }

    #[test]
    fn resolve_ledger_branch_uses_detached_with_head_prefix() {
        let branch = resolve_ledger_branch(Some("HEAD"), Some("aabbccddeeff0011"));
        assert_eq!(branch, "detached@aabbccdd");
    }

    #[test]
    fn resolve_ledger_branch_falls_back_when_head_missing() {
        let branch = resolve_ledger_branch(Some("HEAD"), None);
        assert_eq!(branch, "detached");
    }
}
