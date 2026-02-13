mod parse;
mod transform;

use crate::SessionParser;
use anyhow::Result;
use opensession_core::trace::Session;
use std::path::Path;

pub struct CursorParser;

impl SessionParser for CursorParser {
    fn name(&self) -> &str {
        "cursor"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "vscdb")
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse::parse_cursor_vscdb(path)
    }
}
