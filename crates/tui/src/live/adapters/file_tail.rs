use super::LiveAdapter;
use crate::live::{LiveUpdate, LiveUpdateBatch};
use opensession_core::trace::Session;
use opensession_parsers::SessionParser;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub struct FileTailAdapter {
    path: PathBuf,
    parser: Box<dyn SessionParser>,
    debounce: Duration,
    last_poll_at: Instant,
    last_mtime: Option<SystemTime>,
    last_source_len: Option<u64>,
    known_event_ids: HashSet<String>,
    last_event_count: usize,
    active: bool,
}

impl FileTailAdapter {
    pub fn new(
        path: PathBuf,
        parser: Box<dyn SessionParser>,
        seed_session: &Session,
        debounce: Duration,
    ) -> Self {
        let (last_mtime, last_source_len) = std::fs::metadata(&path)
            .ok()
            .map(|meta| {
                let mtime = meta.modified().ok();
                let len = Some(meta.len());
                (mtime, len)
            })
            .unwrap_or((None, None));

        Self {
            path,
            parser,
            debounce,
            last_poll_at: Instant::now()
                .checked_sub(debounce)
                .unwrap_or_else(Instant::now),
            last_mtime,
            last_source_len,
            known_event_ids: seed_session
                .events
                .iter()
                .map(|event| event.event_id.clone())
                .collect(),
            last_event_count: seed_session.events.len(),
            active: false,
        }
    }

    fn parse_latest_session(&self) -> Option<Session> {
        let session = self.parser.parse(&self.path).ok()?;
        if session.stats.event_count == 0 {
            return None;
        }
        Some(session)
    }

    fn effective_debounce_for_len(&self, source_len: u64) -> Duration {
        let mut debounce = self.debounce;
        if source_len >= 512 * 1024 {
            debounce = debounce.max(Duration::from_millis(900));
        }
        if source_len >= 2 * 1024 * 1024 {
            debounce = debounce.max(Duration::from_millis(1500));
        }
        debounce
    }
}

impl LiveAdapter for FileTailAdapter {
    fn poll(&mut self) -> Option<LiveUpdateBatch> {
        let metadata = std::fs::metadata(&self.path).ok()?;
        let modified = metadata.modified().ok();
        let source_len = metadata.len();
        let debounce = self.effective_debounce_for_len(source_len);
        if self.last_poll_at.elapsed() < debounce {
            return None;
        }
        self.last_poll_at = Instant::now();
        let modified_changed = match (self.last_mtime, modified) {
            (Some(prev), Some(current)) => current > prev,
            (None, Some(_)) => true,
            _ => false,
        };
        let len_changed = self
            .last_source_len
            .map(|prev| prev != source_len)
            .unwrap_or(true);

        if !modified_changed && !len_changed {
            self.active = false;
            return None;
        }

        let latest = self.parse_latest_session()?;
        let appended_events: Vec<_> = latest
            .events
            .iter()
            .filter(|event| !self.known_event_ids.contains(&event.event_id))
            .cloned()
            .collect();
        let event_count_changed = latest.events.len() != self.last_event_count;

        self.last_mtime = modified;
        self.last_source_len = Some(source_len);
        self.known_event_ids = latest
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        self.last_event_count = latest.events.len();

        if appended_events.is_empty() && !event_count_changed {
            self.active = false;
            return None;
        }

        let mut updates = vec![LiveUpdate::SessionReloaded(latest.clone())];
        if !appended_events.is_empty() {
            updates.push(LiveUpdate::EventsAppended(appended_events.clone()));
        }

        let last_event_at = appended_events
            .last()
            .map(|event| event.timestamp)
            .or_else(|| latest.events.last().map(|event| event.timestamp));

        self.active = !appended_events.is_empty();
        Some(LiveUpdateBatch {
            updates,
            cursor: Some(latest.events.len() as u64),
            source_offset: Some(source_len),
            last_event_at,
            active: self.active,
        })
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::FileTailAdapter;
    use crate::live::adapters::LiveAdapter;
    use crate::live::{LiveUpdate, LiveUpdateBatch};
    use chrono::Utc;
    use opensession_core::trace::{Agent, Content, Event, EventType, Session};
    use opensession_parsers::SessionParser;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use std::time::Duration;

    struct DummyParser;

    impl SessionParser for DummyParser {
        fn name(&self) -> &str {
            "dummy"
        }

        fn can_parse(&self, _path: &Path) -> bool {
            true
        }

        fn parse(&self, path: &Path) -> anyhow::Result<Session> {
            let mut session = Session::new(
                "dummy-session".to_string(),
                Agent {
                    provider: "dummy".to_string(),
                    model: "dummy-model".to_string(),
                    tool: "dummy-tool".to_string(),
                    tool_version: None,
                },
            );

            let content = fs::read_to_string(path)?;
            for (idx, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                session.events.push(Event {
                    event_id: format!("event-{idx}"),
                    timestamp: Utc::now(),
                    event_type: EventType::AgentMessage,
                    task_id: None,
                    content: Content::text(line.trim()),
                    duration_ms: None,
                    attributes: std::collections::HashMap::new(),
                });
            }
            session.recompute_stats();
            Ok(session)
        }
    }

    fn make_seed_session() -> Session {
        let mut session = Session::new(
            "dummy-session".to_string(),
            Agent {
                provider: "dummy".to_string(),
                model: "dummy-model".to_string(),
                tool: "dummy-tool".to_string(),
                tool_version: None,
            },
        );
        session.events.push(Event {
            event_id: "event-0".to_string(),
            timestamp: Utc::now(),
            event_type: EventType::AgentMessage,
            task_id: None,
            content: Content::text("seed"),
            duration_ms: None,
            attributes: std::collections::HashMap::new(),
        });
        session.recompute_stats();
        session
    }

    fn write_file(path: &Path, body: &str) {
        let mut file = std::fs::File::create(path).expect("create file");
        file.write_all(body.as_bytes()).expect("write body");
        file.sync_all().expect("sync file");
    }

    #[test]
    fn file_tail_adapter_reports_appended_events() {
        let unique = format!(
            "ops-live-tail-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("session.log");

        write_file(&path, "seed\n");
        let seed = make_seed_session();
        let mut adapter = FileTailAdapter::new(
            path.clone(),
            Box::new(DummyParser),
            &seed,
            Duration::from_millis(0),
        );

        write_file(&path, "seed\nnext\n");
        let batch = adapter.poll().expect("live update batch");
        assert!(batch.has_updates());
        assert!(batch.active);
        assert_eq!(batch.cursor, Some(2));

        let appended = batch
            .updates
            .iter()
            .find_map(|update| match update {
                LiveUpdate::EventsAppended(events) => Some(events),
                _ => None,
            })
            .expect("appended event list");
        assert_eq!(appended.len(), 1);

        fs::remove_file(&path).ok();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_tail_adapter_returns_none_without_source_change() {
        let unique = format!(
            "ops-live-tail-idle-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("session.log");

        write_file(&path, "seed\n");
        let seed = make_seed_session();
        let mut adapter = FileTailAdapter::new(
            path.clone(),
            Box::new(DummyParser),
            &seed,
            Duration::from_millis(0),
        );

        let first = adapter.poll();
        if let Some(LiveUpdateBatch { updates, .. }) = first {
            assert!(updates
                .into_iter()
                .any(|update| matches!(update, LiveUpdate::SessionReloaded(_))));
        }
        let second = adapter.poll();
        assert!(second.is_none());

        fs::remove_file(&path).ok();
        fs::remove_dir_all(&dir).ok();
    }
}
