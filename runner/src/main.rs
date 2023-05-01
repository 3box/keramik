//! Keramic is tool for simluating Ceramic networks
#![deny(warnings)]
#![deny(missing_docs)]

mod bootstrap;
mod scenario;
mod simulate;
mod telemetry;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use opentelemetry::{global, KeyValue};
use opentelemetry::{global::shutdown_tracer_provider, Context};
use tokio;
use tracing::info;

use crate::{bootstrap::bootstrap, simulate::simulate};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(
        long,
        env = "RUNNER_OTLP_ENDPOINT",
        default_value = "http://localhost:4317"
    )]
    otlp_endpoint: String,
}

/// Available Subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Bootstrap peers in the network
    Bootstrap(bootstrap::Opts),
    /// Simulate a load scenario against the network
    Simulate(simulate::Opts),
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Bootstrap(_) => "bootstrap",
            Command::Simulate(_) => "simulate",
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let cx = Context::current();
    let metrics_controller = telemetry::init(args.otlp_endpoint.clone()).await?;

    let meter = global::meter("keramik");
    let runs = meter
        .u64_counter("runner_runs")
        .with_description("Number of runs of the runner")
        .init();

    runs.add(&cx, 1, &[KeyValue::new("command", args.command.name())]);

    info!(?args.command, ?args.otlp_endpoint, "starting runner");
    match args.command {
        Command::Bootstrap(opts) => bootstrap(opts).await?,
        Command::Simulate(opts) => simulate(opts).await?,
    }
    // Flush traces and metrics before shutdown
    shutdown_tracer_provider();
    metrics_controller.stop(&cx)?;
    Ok(())
}
