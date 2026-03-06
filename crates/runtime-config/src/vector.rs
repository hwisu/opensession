use crate::defaults::{
    default_false, default_vector_chunk_overlap_lines, default_vector_chunk_size_lines,
    default_vector_endpoint, default_vector_model, default_vector_top_k_chunks,
    default_vector_top_k_sessions,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub provider: VectorSearchProvider,
    #[serde(default = "default_vector_model")]
    pub model: String,
    #[serde(default = "default_vector_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub granularity: VectorSearchGranularity,
    #[serde(default)]
    pub chunking_mode: VectorChunkingMode,
    #[serde(default = "default_vector_chunk_size_lines")]
    pub chunk_size_lines: u16,
    #[serde(default = "default_vector_chunk_overlap_lines")]
    pub chunk_overlap_lines: u16,
    #[serde(default = "default_vector_top_k_chunks")]
    pub top_k_chunks: u16,
    #[serde(default = "default_vector_top_k_sessions")]
    pub top_k_sessions: u16,
}

impl Default for VectorSearchSettings {
    fn default() -> Self {
        Self {
            enabled: default_false(),
            provider: VectorSearchProvider::default(),
            model: default_vector_model(),
            endpoint: default_vector_endpoint(),
            granularity: VectorSearchGranularity::default(),
            chunking_mode: VectorChunkingMode::default(),
            chunk_size_lines: default_vector_chunk_size_lines(),
            chunk_overlap_lines: default_vector_chunk_overlap_lines(),
            top_k_chunks: default_vector_top_k_chunks(),
            top_k_sessions: default_vector_top_k_sessions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorSearchProvider {
    #[default]
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorSearchGranularity {
    #[default]
    EventLineChunk,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorChunkingMode {
    #[default]
    Auto,
    Manual,
}
