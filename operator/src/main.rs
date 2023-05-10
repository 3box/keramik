use std::sync::Arc;

use anyhow::Result;
use clap::{command, Parser, Subcommand};
use futures::stream::StreamExt;
use kube::runtime::watcher::Config;
use kube::{client::Client, runtime::Controller, Api};
use opentelemetry::{global::shutdown_tracer_provider, Context};
use tracing::{debug, error};

use keramik_operator::network::{on_error, reconcile, ContextData, Network};

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
    // Init env_logger for deps that use log.
    // TODO should we use tracing_log instead?
    env_logger::init();

    let args = Cli::parse();
    let metrics_controller = keramik_common::telemetry::init(args.otlp_endpoint.clone()).await?;

    match args.command {
        Command::Daemon => {
            let k_client: Client = Client::try_default().await.unwrap();

            // Add api for other resources, ie ceramic nodes
            let net_api: Api<Network> = Api::all(k_client.clone());
            let context: Arc<ContextData> = Arc::new(ContextData::new(k_client.clone()));

            Controller::new(net_api.clone(), Config::default())
                .run(reconcile, on_error, context)
                .for_each(|rec_res| async move {
                    match rec_res {
                        Ok(network_resource) => {
                            debug!("Success: {:?}", network_resource);
                        }
                        Err(rec_err) => {
                            error!("Fail: {:?}", rec_err)
                        }
                    }
                })
                .await;
        }
    };

    // Flush traces and metrics before shutdown
    shutdown_tracer_provider();
    let cx = Context::default();
    metrics_controller.stop(&cx)?;

    Ok(())
}
