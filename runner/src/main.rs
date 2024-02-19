//! Runner is a short lived process that performs various tasks within a Ceramic network.
#![deny(missing_docs)]

mod bootstrap;
mod scenario;
mod simulate;
mod utils;

use keramik_common::telemetry;

use anyhow::Result;
use clap::{Parser, Subcommand};
use opentelemetry::global::{shutdown_meter_provider, shutdown_tracer_provider};
use opentelemetry::{global, KeyValue};
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

/// Result of a command.
/// This is primarily used to differentiate between a simulation failure and a failure to run the command.
/// Both should return a non-zero exit code, but the former should still report and clean up since the command
/// executed successfully, it just didn't pass thresholds or whatever correctness requirements we enforce.
#[derive(Debug)]
pub enum CommandResult {
    /// Command completed successfully
    Success,
    /// Command failed
    Failure(anyhow::Error),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    telemetry::init_tracing(Some(args.otlp_endpoint.clone())).await?;
    let metrics_controller = telemetry::init_metrics_otlp(args.otlp_endpoint.clone()).await?;
    info!("starting runner");

    let meter = global::meter("keramik");
    let runs = meter
        .u64_counter("runner_runs")
        .with_description("Number of runs of the runner")
        .init();

    runs.add(1, &[KeyValue::new("command", args.command.name())]);

    info!(?args.command, ?args.otlp_endpoint, "starting runner");
    let success = match args.command {
        Command::Bootstrap(opts) => bootstrap(opts).await?,
        Command::Simulate(opts) => simulate(opts).await?,
        Command::Noop => CommandResult::Success,
    };

    // Flush traces and metrics before shutdown
    shutdown_tracer_provider();
    metrics_controller.force_flush()?;
    drop(metrics_controller);
    shutdown_meter_provider();

    // This fixes lost metrics not sure why :(
    // Seems to be related to the inflight gRPC request getting cancelled
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    match success {
        CommandResult::Success => Ok(()),
        CommandResult::Failure(e) => Err(e),
    }
}
