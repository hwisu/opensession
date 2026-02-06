use crate::SessionParser;
use anyhow::Result;
use opensession_core::trace::Session;
use std::path::Path;

pub struct AiderParser;

impl SessionParser for AiderParser {
    fn name(&self) -> &str {
        "aider"
    }

    fn can_parse(&self, path: &Path) -> bool {
        path.file_name()
            .is_some_and(|f| f.to_str().is_some_and(|s| s.contains(".aider.chat.history")))
    }

    fn parse(&self, _path: &Path) -> Result<Session> {
        anyhow::bail!("Aider parser not yet implemented")
    }
}
