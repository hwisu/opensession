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
}
