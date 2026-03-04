use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HailCompactFileChange {
    pub path: String,
    pub layer: String,
    pub operation: String,
    pub lines_added: u64,
    pub lines_removed: u64,
}
