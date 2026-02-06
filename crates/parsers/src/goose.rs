use crate::SessionParser;
use anyhow::Result;
use opensession_core::trace::Session;
use std::path::Path;

pub struct GooseParser;

impl SessionParser for GooseParser {
    fn name(&self) -> &str {
        "goose"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.to_str()
            .is_some_and(|s| s.contains("goose") && s.ends_with(".db"))
    }

    fn parse(&self, _path: &Path) -> Result<Session> {
        anyhow::bail!("Goose parser not yet implemented")
    }
}
