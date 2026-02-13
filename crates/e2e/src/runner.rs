use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinSet;

use crate::specs;

/// Result of running a single spec.
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration: Duration,
    pub error: Option<String>,
}

/// Aggregated results of a full test run.
pub struct TestSuite {
    pub results: Vec<TestResult>,
}

impl TestSuite {
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| !r.passed).count()
    }

    pub fn total(&self) -> usize {
        self.results.len()
    }
}

/// Run all E2E specs in parallel.
///
/// - `filter`: optional substring filter on spec names.
/// - `include_docker_only`: include specs that only work against Docker (e.g. admin-gated team creation).
pub async fn run_all(
    ctx: Arc<crate::client::TestContext>,
    filter: Option<&str>,
    include_docker_only: bool,
) -> TestSuite {
    let mut set = JoinSet::new();

    macro_rules! spawn_spec {
        ($module:ident :: $name:ident) => {
            let spec_name = concat!(stringify!($module), "::", stringify!($name));
            if filter.map_or(true, |f| spec_name.contains(f)) {
                let ctx = ctx.clone();
                set.spawn(async move {
                    let start = Instant::now();
                    let result = specs::$module::$name(&ctx).await;
                    let duration = start.elapsed();
                    TestResult {
                        name: spec_name.to_string(),
                        passed: result.is_ok(),
                        duration,
                        error: result.err().map(|e| format!("{e:#}")),
                    }
                });
            }
        };
    }

    crate::for_each_spec!(spawn_spec);
    if include_docker_only {
        crate::for_each_docker_only_spec!(spawn_spec);
    }

    let mut results = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(r) => results.push(r),
            Err(e) => results.push(TestResult {
                name: "unknown (join error)".into(),
                passed: false,
                duration: Duration::ZERO,
                error: Some(format!("{e:#}")),
            }),
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    TestSuite { results }
}
