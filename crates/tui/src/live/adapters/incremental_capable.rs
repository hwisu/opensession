use super::{FileTailAdapter, LiveAdapter};
use crate::live::{LiveUpdate, LiveUpdateBatch};
use opensession_core::trace::Session;
use opensession_parsers::incremental::IncrementalParser;
use opensession_parsers::SessionParser;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

struct IncrementalState {
    parser: IncrementalParser,
    session: Session,
    cursor: u64,
    debounce: Duration,
    last_poll_at: Instant,
    last_mtime: Option<SystemTime>,
    active: bool,
}

impl IncrementalState {
    fn bootstrap(path: &PathBuf, seed_session: &Session, debounce: Duration) -> Option<Self> {
        let mut parser = IncrementalParser::new();
        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            let _ = parser.parse_line(&line);
        }

        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            parser,
            session: seed_session.clone(),
            cursor: metadata.len(),
            debounce,
            last_poll_at: Instant::now()
                .checked_sub(debounce)
                .unwrap_or_else(Instant::now),
            last_mtime: metadata.modified().ok(),
            active: false,
        })
    }
}

/// Adapter wrapper for parsers that support optional incremental ingestion.
///
/// For claude-code JSONL we parse appended lines incrementally; all other
/// sources use full-parse diff fallback via `FileTailAdapter`.
pub struct IncrementalCapableAdapter {
    inner: FileTailAdapter,
    path: PathBuf,
    #[cfg_attr(not(test), allow(dead_code))]
    incremental_supported: bool,
    incremental_state: Option<IncrementalState>,
}

impl IncrementalCapableAdapter {
    pub fn new(
        path: PathBuf,
        parser: Box<dyn SessionParser>,
        seed_session: &Session,
        debounce: Duration,
    ) -> Self {
        let parser_name = parser.name().to_ascii_lowercase();
        let incremental_supported = parser_name == "claude-code"
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"));

        let incremental_state = if incremental_supported {
            IncrementalState::bootstrap(&path, seed_session, debounce)
        } else {
            None
        };

        Self {
            inner: FileTailAdapter::new(path.clone(), parser, seed_session, debounce),
            path,
            incremental_supported,
            incremental_state,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn incremental_supported(&self) -> bool {
        self.incremental_supported
    }

    fn poll_incremental(&mut self) -> Option<LiveUpdateBatch> {
        let state = self.incremental_state.as_mut()?;
        if state.last_poll_at.elapsed() < state.debounce {
            return None;
        }
        state.last_poll_at = Instant::now();

        let metadata = std::fs::metadata(&self.path).ok()?;
        let modified = metadata.modified().ok();
        let source_len = metadata.len();
        let modified_changed = match (state.last_mtime, modified) {
            (Some(prev), Some(current)) => current > prev,
            (None, Some(_)) => true,
            _ => false,
        };
        let len_changed = source_len != state.cursor;

        if !modified_changed && !len_changed {
            state.active = false;
            return None;
        }

        // Rewrite/truncate/no append pattern -> fall back to full parse diff.
        if source_len <= state.cursor {
            return self.inner.poll();
        }

        let mut file = File::open(&self.path).ok()?;
        if file.seek(SeekFrom::Start(state.cursor)).is_err() {
            return self.inner.poll();
        }
        let mut reader = BufReader::new(file);
        let mut consumed_bytes = 0u64;
        let mut line = String::new();
        let mut appended_events = Vec::new();

        loop {
            line.clear();
            let read = match reader.read_line(&mut line) {
                Ok(n) => n,
                Err(_) => return self.inner.poll(),
            };
            if read == 0 {
                break;
            }
            consumed_bytes = consumed_bytes.saturating_add(read as u64);
            match state.parser.parse_line(&line) {
                Ok(mut events) => appended_events.append(&mut events),
                Err(_) => return self.inner.poll(),
            }
        }

        state.cursor = state.cursor.saturating_add(consumed_bytes);
        state.last_mtime = modified;

        if appended_events.is_empty() {
            state.active = false;
            return None;
        }

        state.session.events.extend(appended_events.clone());
        state.session.recompute_stats();
        if let Some(last) = state.session.events.last() {
            state.session.context.updated_at = last.timestamp;
        }

        state.active = true;
        Some(LiveUpdateBatch {
            updates: vec![
                LiveUpdate::SessionReloaded(Box::new(state.session.clone())),
                LiveUpdate::EventsAppended(appended_events.clone()),
            ],
            cursor: Some(state.session.events.len() as u64),
            source_offset: Some(state.cursor),
            last_event_at: appended_events.last().map(|event| event.timestamp),
            active: true,
        })
    }
}

impl LiveAdapter for IncrementalCapableAdapter {
    fn poll(&mut self) -> Option<LiveUpdateBatch> {
        if self.incremental_supported {
            if let Some(batch) = self.poll_incremental() {
                return Some(batch);
            }
        }
        self.inner.poll()
    }

    fn is_active(&self) -> bool {
        if let Some(state) = self.incremental_state.as_ref() {
            state.active || self.inner.is_active()
        } else {
            self.inner.is_active()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IncrementalCapableAdapter;
    use opensession_core::trace::{Agent, Session};
    use opensession_parsers::SessionParser;
    use std::path::Path;
    use std::time::Duration;

    struct ClaudeParser;

    impl SessionParser for ClaudeParser {
        fn name(&self) -> &str {
            "claude-code"
        }

        fn can_parse(&self, _path: &Path) -> bool {
            true
        }

        fn parse(&self, _path: &Path) -> anyhow::Result<Session> {
            Ok(Session::new(
                "seed".to_string(),
                Agent {
                    provider: "anthropic".to_string(),
                    model: "claude".to_string(),
                    tool: "claude-code".to_string(),
                    tool_version: None,
                },
            ))
        }
    }

    #[test]
    fn marks_claude_jsonl_as_incremental_capable() {
        let seed = Session::new(
            "seed".to_string(),
            Agent {
                provider: "anthropic".to_string(),
                model: "claude".to_string(),
                tool: "claude-code".to_string(),
                tool_version: None,
            },
        );
        let adapter = IncrementalCapableAdapter::new(
            Path::new("/tmp/sample.jsonl").to_path_buf(),
            Box::new(ClaudeParser),
            &seed,
            Duration::from_millis(0),
        );

        assert!(adapter.incremental_supported());
    }
}
