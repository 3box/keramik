//! Operator is a long lived process that auotmates creating and managing Ceramic networks.
#![deny(missing_docs)]
use anyhow::Result;
use clap::{arg, command, Parser, Subcommand, ValueEnum};
use keramik_common::telemetry;
use keramik_operator::set_network_log_format;
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

    #[arg(long, env = "OPERATOR_LOG_FORMAT")]
    log_format: Option<LogFormat>,
}

#[derive(ValueEnum, Debug, Clone)]
/// The format of the logs
pub enum LogFormat {
    /// Compact single line logs
    SingleLine,
    /// Pretty multi-line logs
    MultiLine,
    /// JSON logs
    Json,
}

impl From<LogFormat> for telemetry::LogFormat {
    fn from(format: LogFormat) -> telemetry::LogFormat {
        match format {
            LogFormat::SingleLine => telemetry::LogFormat::SingleLine,
            LogFormat::MultiLine => telemetry::LogFormat::MultiLine,
            LogFormat::Json => telemetry::LogFormat::Json,
        }
    }
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
    let log_format = args
        .log_format
        .map(telemetry::LogFormat::from)
        .unwrap_or_default();
    set_network_log_format(log_format);
    telemetry::init_tracing(args.otlp_endpoint, log_format).await?;

    let (metrics_controller, metrics_server_shutdown, metrics_server_join) =
        telemetry::init_metrics_prom(&args.prom_bind.parse()?).await?;
    info!("starting operator");

    match args.command {
        Command::Daemon => {
            tokio::join!(
                keramik_operator::network::run(),
                keramik_operator::simulation::run()
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
