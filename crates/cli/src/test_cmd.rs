use std::sync::Arc;

use clap::Args;

use opensession_e2e::{client::TestContext, runner};

#[derive(Args)]
pub struct TestArgs {
    /// Target: server, worker, or custom
    #[arg(long, default_value = "server")]
    pub target: String,

    /// Custom base URL (overrides --target)
    #[arg(long)]
    pub base_url: Option<String>,

    /// Filter specs by substring
    #[arg(long)]
    pub filter: Option<String>,
}

pub async fn run_test(args: TestArgs) -> anyhow::Result<()> {
    let base_url = args.base_url.unwrap_or_else(|| match args.target.as_str() {
        "server" | "docker" => "http://localhost:3000".into(),
        "worker" => "https://opensession.io".into(),
        other => other.into(),
    });

    eprintln!("Running E2E tests against {base_url}");

    let ctx = Arc::new(TestContext::new(base_url));
    let suite = runner::run_all(ctx, args.filter.as_deref()).await;

    for r in &suite.results {
        let icon = if r.passed { "PASS" } else { "FAIL" };
        let dur = format!("{:.0}ms", r.duration.as_secs_f64() * 1000.0);
        eprintln!("  {icon} {name} ({dur})", name = r.name);
        if let Some(ref err) = r.error {
            eprintln!("       {err}");
        }
    }

    eprintln!(
        "\n{} passed, {} failed, {} total",
        suite.passed(),
        suite.failed(),
        suite.total()
    );

    if suite.failed() > 0 {
        std::process::exit(1);
    }

    Ok(())
}
