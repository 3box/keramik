//! Keramic is tool for simluating Ceramic networks
#![deny(warnings)]
#![deny(missing_docs)]

mod bootstrap;
mod scenario;
mod simulate;
mod utils;

use keramik_common::telemetry;

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
    /// Do nothing and exit
    Noop,
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Bootstrap(_) => "bootstrap",
            Command::Simulate(_) => "simulate",
            Command::Noop => "noop",
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Init env_logger for deps that use log.
    // TODO should we use tracing_log instead?
    env_logger::init();

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
        Command::Noop => {}
    }

    // Flush traces and metrics before shutdown
    shutdown_tracer_provider();
    metrics_controller.stop(&cx)?;

    // This fixes lost metrics not sure why :(
    // Seems to be related to the inflight gRPC request getting cancelled
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(())
}
