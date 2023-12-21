use std::{io::Cursor, path::PathBuf, time::Duration};

use anyhow::Result;
use ceramic_pocket_knife::cli::{
    Command, EventIdGenerateArgs, Network, StreamIdGenerateArgs, StreamType,
};
use clap::Args;
use futures::StreamExt;
use keramik_common::peer_info::{IpfsPeerInfo, Peer};
use serde::Serialize;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook_tokio::Signals;
use tdigest::TDigest;
use tokio::{select, time::Instant};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{error, info};

use crate::utils::parse_peers_info;

/// Options to Simulate command
#[derive(Args, Debug)]
pub struct Opts {
    /// Path to file containing the list of peers.
    /// File should contian JSON encoding of Vec<Peer>.
    #[arg(long, env = "CAS_PUSH_PEERS_PATH")]
    peers: PathBuf,

    /// Number of concurrent requests to each peer.
    #[arg(long, default_value_t = 2, env = "CAS_PUSH_CONCURRENCY")]
    concurrency: usize,

    /// Maximum number of requests per second per thread/task.
    #[arg(long, default_value_t = 10, env = "CAS_PUSH_THROTTLE")]
    throttle: usize,
}

pub async fn cas_push(opts: Opts) -> Result<()> {
    let mut peers: Vec<IpfsPeerInfo> = parse_peers_info(opts.peers)
        .await?
        .into_iter()
        .map(|peer| match peer {
            Peer::Ceramic(info) => info.into(),
            Peer::Ipfs(info) => info,
        })
        .collect();
    info!(?peers, "peers");
    let psuedo_cas = peers.swap_remove(peers.len() - 1);
    info!(?psuedo_cas, "psuedo_cas");

    let tracker = TaskTracker::new();
    let cancellation = CancellationToken::new();

    let mut signals = Signals::new([SIGHUP, SIGTERM, SIGINT, SIGQUIT])?;
    let shutdown = cancellation.clone();
    tracker.spawn(async move {
        if let Some(signal) = signals.next().await {
            info!(?signal, "signal received");
            shutdown.cancel();
        }
    });

    let mut models = Vec::new();
    let mut digest_handles = Vec::new();
    for _ in 0..opts.concurrency {
        for peer in &peers {
            let model = generate_model_id().await?;
            info!(peer = %peer.peer_id, model, "new model");
            models.push(model.clone());
            subscribe(peer.ipfs_rpc_addr.clone(), &model).await;
            digest_handles.push(tracker.spawn(spam(
                peer.ipfs_rpc_addr.clone() + "/ceramic/events",
                model,
                opts.throttle,
                cancellation.clone(),
            )));
        }
    }

    info!("peer spam started");

    // Start psudeo cas time events
    let num_models = models.len();
    for model in models {
        subscribe(psuedo_cas.ipfs_rpc_addr.clone(), &model).await;
        digest_handles.push(tracker.spawn(spam(
            psuedo_cas.ipfs_rpc_addr.clone() + "/ceramic/events",
            model,
            opts.throttle / num_models,
            cancellation.clone(),
        )));
    }
    info!("psuedo_cas spam started");
    tracker.close();

    tracker.wait().await;

    let mut digests = Vec::with_capacity(digest_handles.len());
    for d in digest_handles {
        digests.push(d.await?);
    }
    let digest = TDigest::merge_digests(digests);
    info!(
        p99 = digest.estimate_quantile(0.99),
        p90 = digest.estimate_quantile(0.90),
        p75 = digest.estimate_quantile(0.75),
        p50 = digest.estimate_quantile(0.50),
        total = digest.count(),
        "final durations"
    );

    Ok(())
}

async fn generate_model_id() -> Result<String> {
    let stdin: &[u8] = &[];
    let mut stdout = Vec::new();
    ceramic_pocket_knife::run(
        ceramic_pocket_knife::Cli {
            command: Command::StreamIdGenerate(StreamIdGenerateArgs {
                r#type: StreamType::Model,
            }),
        },
        stdin,
        Cursor::new(&mut stdout),
    )
    .await?;
    Ok(String::from_utf8(stdout)?)
}
async fn generate_event_id(model: String) -> Result<String> {
    let stdin: &[u8] = &[];
    let mut stdout = Vec::new();
    ceramic_pocket_knife::run(
        ceramic_pocket_knife::Cli {
            command: Command::EventIdGenerate(EventIdGenerateArgs {
                network: Network::Local,
                local_network_id: Some(0),
                sort_key: Some("model".to_string()),
                sort_value: Some(model),
                controller: None,
                init_id: None,
            }),
        },
        stdin,
        Cursor::new(&mut stdout),
    )
    .await?;
    Ok(String::from_utf8(stdout)?)
}

async fn subscribe(base_url: String, model: &str) {
    let client = reqwest::Client::new();
    if let Err(err) = client
        .get(&format!(
            "{base_url}/ceramic/subscribe/model/{model}?limit=1"
        ))
        .send()
        .await
    {
        error!(?err, "failed to do post request:");
    }
}
async fn spam(
    url: String,
    model: String,
    throttle: usize,
    cancellation: CancellationToken,
) -> TDigest {
    info!(url, throttle, model, "spaming");

    let mut ticker = tokio::time::interval(Duration::from_secs(1));

    const BUFFER: usize = 1000;
    let mut durations = Vec::with_capacity(BUFFER);
    let mut digest = TDigest::new_with_size(durations.capacity());
    let client = reqwest::Client::new();
    #[derive(Serialize)]
    struct PostBody {
        #[serde(rename = "eventId")]
        event_id: String,
    }
    'outer: loop {
        select! {
            _ = cancellation.cancelled() => {
                break
            }
            _ = ticker.tick() => {
            }
        };
        for _ in 0..throttle {
            let mut event_id = generate_event_id(model.clone()).await.unwrap();
            event_id.insert_str(0, "F");
            let now = Instant::now();
            select! {
                _ = cancellation.cancelled() => {
                    break 'outer;
                }
                res = client.post(&url).json(&PostBody { event_id }).send() => {
                    if let Err(err) = res {
                        error!(?err, "failed to do post request:");
                    }
                }
            };
            durations.push(now.elapsed().as_secs_f64());
            if durations.len() == durations.capacity() {
                digest = digest.merge_unsorted(durations);
                durations = Vec::with_capacity(BUFFER);
            }
        }
    }
    digest.merge_unsorted(durations)
}
