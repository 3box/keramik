//! Operator is a long lived process that auotmates creating and managing Ceramic networks.
#![deny(missing_docs)]
use anyhow::Result;
use clap::{command, Parser, Subcommand};
use opentelemetry::{global::shutdown_tracer_provider, Context};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(
        long,
        env = "OPERATOR_OTLP_ENDPOINT",
        default_value = "http://localhost:4317"
    )]
    otlp_endpoint: String,
}

/// Available Subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the daemon
    Daemon,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_log::LogTracer::init()?;

    let args = Cli::parse();
    let metrics_controller = keramik_common::telemetry::init(args.otlp_endpoint.clone()).await?;

    match args.command {
        Command::Daemon => {
            tokio::join!(
                keramik_operator::network::run(),
                keramik_operator::simulation::run()
            );
        }
    };

    // Flush traces and metrics before shutdown
    shutdown_tracer_provider();
    let cx = Context::default();
    metrics_controller.stop(&cx)?;

    Ok(())
}
