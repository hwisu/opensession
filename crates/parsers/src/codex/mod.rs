use crate::SessionParser;
use crate::common::{
    INTERACTIVE_USER_INPUT_TOOL, attach_semantic_attrs, attach_source_attrs, canonical_tool_name,
    infer_tool_kind, set_first,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use opensession_core::session::{ATTR_PARENT_SESSION_ID, ATTR_SESSION_ROLE};
#[allow(unused_imports)]
use opensession_core::trace::{
    Agent, Content, ContentBlock, Event, EventType, Session, SessionContext,
};
#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap};
#[allow(unused_imports)]
use std::io::BufRead;
use std::path::{Path, PathBuf};

mod dedupe;
mod detect;
mod helpers;
mod interactive;
mod metrics;
mod parse;
mod transform;

#[cfg(test)]
mod tests;

pub struct CodexParser;

impl SessionParser for CodexParser {
    fn name(&self) -> &str {
        "codex"
    }

    fn can_parse(&self, path: &Path) -> bool {
        detect::can_parse_codex_path(path)
    }

    fn parse(&self, path: &Path) -> Result<Session> {
        parse_codex_jsonl(path)
    }
}

#[allow(unused_imports)]
pub(super) use dedupe::*;
#[allow(unused_imports)]
pub(super) use helpers::*;
#[allow(unused_imports)]
pub(super) use interactive::*;
#[allow(unused_imports)]
pub(super) use metrics::*;
#[allow(unused_imports)]
pub(super) use parse::parse_codex_jsonl;
#[allow(unused_imports)]
pub(super) use transform::*;
