-- Local semantic summary cache (App + CLI + Daemon)
CREATE TABLE IF NOT EXISTS session_semantic_summaries (
    session_id          TEXT PRIMARY KEY,
    summary_json        TEXT NOT NULL,
    generated_at        TEXT NOT NULL,
    provider            TEXT NOT NULL,
    model               TEXT,
    source_kind         TEXT NOT NULL,
    generation_kind     TEXT NOT NULL,
    prompt_fingerprint  TEXT,
    source_details_json TEXT,
    diff_tree_json      TEXT,
    error               TEXT,
    updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_session_semantic_summaries_generated_at
    ON session_semantic_summaries(generated_at DESC);
