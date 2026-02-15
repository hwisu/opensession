pub mod adapters;
pub mod state;

use chrono::{DateTime, Utc};
use opensession_core::trace::{Event, Session};
use opensession_parsers::all_parsers;
use std::path::Path;
use std::time::Duration;

pub use state::FollowTailState;

#[derive(Debug, Clone)]
pub enum LiveUpdate {
    SessionReloaded(Session),
    EventsAppended(Vec<Event>),
}

#[derive(Debug, Clone)]
pub struct LiveUpdateBatch {
    pub updates: Vec<LiveUpdate>,
    pub cursor: Option<u64>,
    pub source_offset: Option<u64>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub active: bool,
}

impl LiveUpdateBatch {
    pub fn has_updates(&self) -> bool {
        !self.updates.is_empty()
    }
}

pub trait LiveFeedProvider: Send + Sync {
    fn subscribe(
        &self,
        path: &Path,
        seed_session: &Session,
        debounce: Duration,
    ) -> Option<LiveSubscription>;
}

#[derive(Default)]
pub struct DefaultLiveFeedProvider;

impl LiveFeedProvider for DefaultLiveFeedProvider {
    fn subscribe(
        &self,
        path: &Path,
        seed_session: &Session,
        debounce: Duration,
    ) -> Option<LiveSubscription> {
        let parser = all_parsers().into_iter().find(|p| p.can_parse(path))?;
        let parser_name = parser.name().to_ascii_lowercase();

        let adapter: Box<dyn adapters::LiveAdapter> = if parser_name == "claude-code" {
            Box::new(adapters::IncrementalCapableAdapter::new(
                path.to_path_buf(),
                parser,
                seed_session,
                debounce,
            ))
        } else {
            Box::new(adapters::FileTailAdapter::new(
                path.to_path_buf(),
                parser,
                seed_session,
                debounce,
            ))
        };

        Some(LiveSubscription { adapter })
    }
}

pub struct LiveSubscription {
    adapter: Box<dyn adapters::LiveAdapter>,
}

impl LiveSubscription {
    pub fn poll_update(&mut self) -> Option<LiveUpdateBatch> {
        self.adapter.poll()
    }

    pub fn is_active(&self) -> bool {
        self.adapter.is_active()
    }
}
