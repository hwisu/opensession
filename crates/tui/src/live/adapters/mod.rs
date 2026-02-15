mod file_tail;
mod incremental_capable;

use crate::live::LiveUpdateBatch;

pub use file_tail::FileTailAdapter;
pub use incremental_capable::IncrementalCapableAdapter;

pub trait LiveAdapter: Send {
    fn poll(&mut self) -> Option<LiveUpdateBatch>;
    fn is_active(&self) -> bool;
}
