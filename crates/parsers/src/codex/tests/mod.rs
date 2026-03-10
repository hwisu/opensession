pub(super) use super::*;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn temp_test_dir(prefix: &str) -> PathBuf {
    let sequence = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{sequence}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|_| panic!("create temp dir for {}", dir.display()));
    dir
}

mod config;
mod desktop_a;
mod desktop_b;
mod interaction;
