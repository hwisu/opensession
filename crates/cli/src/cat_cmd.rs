use anyhow::{Context, Result};
use clap::Args;
use opensession_core::source_uri::SourceUri;
use opensession_local_store::read_local_object_from_uri;

#[derive(Debug, Clone, Args)]
pub struct CatArgs {
    /// Local source URI (`os://src/local/<sha256>`).
    pub uri: String,
}

pub fn run(args: CatArgs) -> Result<()> {
    let uri = SourceUri::parse(&args.uri)?;
    let cwd = std::env::current_dir().context("read current directory")?;
    let (_path, bytes) = read_local_object_from_uri(&uri, &cwd)?;
    print!("{}", String::from_utf8_lossy(&bytes));
    Ok(())
}
