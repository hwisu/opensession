use std::sync::Arc;

use clap::Args;

use opensession_e2e::{client::TestContext, runner};

#[derive(Args)]
pub struct TestArgs {
    /// Target: docker, worker, or custom
    #[arg(long, default_value = "docker")]
    pub target: String,

    /// Custom base URL (overrides --target)
    #[arg(long)]
    pub base_url: Option<String>,

    /// Filter specs by substring
    #[arg(long)]
    pub filter: Option<String>,

    /// Admin email (required for worker target)
    #[arg(long)]
    pub admin_email: Option<String>,

    /// Admin password (required for worker target)
    #[arg(long)]
    pub admin_password: Option<String>,
}

pub async fn run_test(args: TestArgs) -> anyhow::Result<()> {
    let base_url = args.base_url.unwrap_or_else(|| match args.target.as_str() {
        "docker" => "http://localhost:3000".into(),
        "worker" => "https://opensession.io".into(),
        other => other.into(),
    });

    eprintln!("Running E2E tests against {base_url}");

    let ctx = TestContext::new(base_url);
    let include_docker_only = args.target != "worker";

    match args.target.as_str() {
        "worker" => {
            let email = args
                .admin_email
                .or_else(|| std::env::var("E2E_ADMIN_EMAIL").ok())
                .expect("--admin-email or E2E_ADMIN_EMAIL required for worker target");
            let password = args
                .admin_password
                .or_else(|| std::env::var("E2E_ADMIN_PASSWORD").ok())
                .expect("--admin-password or E2E_ADMIN_PASSWORD required for worker target");
            ctx.setup_admin_with_credentials(&email, &password).await?;
        }
        _ => {
            ctx.setup_admin().await?;
        }
    }

    let ctx = Arc::new(ctx);
    let suite = runner::run_all(ctx, args.filter.as_deref(), include_docker_only).await;

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
