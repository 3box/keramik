//! Keramic is tool for simluating Ceramic networks
#![deny(warnings)]
#![deny(missing_docs)]

mod bootstrap;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio;
use tracing_subscriber::{self, EnvFilter};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Available Subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Bootstrap peer for a specific node
    Bootstrap(bootstrap::Opts),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_ansi(false)
        .init();

    let args = Cli::parse();
    match args.command {
        Command::Bootstrap(opts) => bootstrap::run(opts).await?,
    }
    Ok(())
}
