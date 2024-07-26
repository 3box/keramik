//! Runner is a short lived process that performs various tasks within a Ceramic network.
#![deny(missing_docs)]

mod bootstrap;
mod load_generator;
mod scenario;
mod simulate;
mod utils;

use crate::gen::simulate_load;
use crate::{bootstrap::bootstrap, load_generator::gen, simulate::simulate};
use anyhow::Result;
use clap::{Parser, Subcommand};
use keramik_common::telemetry;
use opentelemetry::global::{shutdown_meter_provider, shutdown_tracer_provider};
use opentelemetry::{global, KeyValue};
use tracing::info;

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
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Bootstrap peers in the network
    Bootstrap(bootstrap::Opts),
    /// Simulate a load scenario against the network
    Simulate(simulate::Opts),
    /// Do nothing and exit
    Noop,
    /// Generate load, currently this command is not used
    GenerateLoad(gen::WeekLongSimulationOpts),
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Bootstrap(_) => "bootstrap",
            Command::Simulate(_) => "simulate",
            Command::Noop => "noop",
            Command::GenerateLoad(_) => "generate-load",
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

// TODO : Enable metrics/tracing for load generator command
// Metrics and tracing have been disabled for load generator due to memory issues.
// Memory grows in the runner when this is enabled not making it live long enough to finish the load generation
#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if !matches!(args.command, Command::GenerateLoad(_)) {
        telemetry::init_tracing(Some(args.otlp_endpoint.clone())).await?;
    }
    let metrics_controller = if matches!(args.command, Command::GenerateLoad(_)) {
        None
    } else {
        Some(telemetry::init_metrics_otlp(args.otlp_endpoint.clone()).await?)
    };
    info!("starting runner");

    let meter = global::meter("keramik");
    let runs = meter
        .u64_counter("runner_runs")
        .with_description("Number of runs of the runner")
        .init();

    runs.add(1, &[KeyValue::new("command", args.command.name())]);

    info!(?args.command, ?args.otlp_endpoint, "starting runner");
    let success = match args.command.clone() {
        Command::Bootstrap(opts) => bootstrap(opts).await?,
        Command::Simulate(opts) => simulate(opts).await?,
        Command::GenerateLoad(opts) => simulate_load(opts).await?,
        Command::Noop => CommandResult::Success,
    };
    if !matches!(args.command, Command::GenerateLoad(_)) {
        // Flush traces and metrics before shutdown
        shutdown_tracer_provider();
        if let Some(metrics_controller) = metrics_controller.clone() {
            metrics_controller.force_flush()?;
        }
        drop(metrics_controller);
        shutdown_meter_provider();
    }

    // This fixes lost metrics not sure why :(
    // Seems to be related to the inflight gRPC request getting cancelled
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    match success {
        CommandResult::Success => Ok(()),
        CommandResult::Failure(e) => Err(e),
    }
}
