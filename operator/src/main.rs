//! Operator is a long lived process that automates creating and managing Ceramic networks.
#![deny(missing_docs)]
use anyhow::Result;
use clap::{command, Parser, Subcommand};
use keramik_common::telemetry;
use opentelemetry::global::{shutdown_meter_provider, shutdown_tracer_provider};
use tracing::info;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, env = "OPERATOR_OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,

    #[arg(long, env = "OPERATOR_PROM_BIND", default_value = "0.0.0.0:9464")]
    prom_bind: String,
}

/// Available Subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the daemon
    Daemon,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    telemetry::init_tracing(args.otlp_endpoint).await?;
    let (metrics_controller, metrics_server_shutdown, metrics_server_join) =
        telemetry::init_metrics_prom(&args.prom_bind.parse()?).await?;
    info!("starting operator");

    match args.command {
        Command::Daemon => {
            tokio::join!(
                keramik_operator::network::run(),
                keramik_operator::simulation::run(),
                keramik_operator::lgen::run(),
            );
        }
    };

    // Shutdown the metrics server
    let _ = metrics_server_shutdown.send(());
    metrics_server_join.await??;

    // Flush traces and metrics before shutdown
    shutdown_tracer_provider();
    metrics_controller.force_flush()?;
    drop(metrics_controller);
    shutdown_meter_provider();

    Ok(())
}
