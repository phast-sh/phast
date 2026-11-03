//! The `phast` executable.
//!
//! Deliberately minimal. Code placed here is reachable only by running the binary,
//! so anything with behavior worth testing or reusing belongs in `phast-cli`, and
//! this crate stays a shell around it. The tokio runtime is set up here because every
//! command handler is async.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    phast_cli::run().await
}
