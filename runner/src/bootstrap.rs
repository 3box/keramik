use anyhow::Result;
use clap::{Args, ValueEnum};
use rand::seq::IteratorRandom;
use tracing::{debug, error};

use crate::utils::{all_peer_addrs, connect_peers};

/// Options to Bootstrap command
#[derive(Args, Debug)]
pub struct Opts {
    /// Bootstrap method to use.
    #[arg(long, value_enum, default_value_t, env = "BOOTSTRAP_METHOD")]
    method: Method,

    /// Number of peers to connecto to.
    #[arg(long, env = "BOOTSTRAP_N")]
    n: usize,

    /// Total number of peers in the network
    #[arg(long, env = "BOOTSTRAP_TOTAL")]
    total: usize,
}

#[derive(Clone, Debug, ValueEnum)]
enum Method {
    /// Connects to next N peers
    Ring,
    /// Connects to N peers at random.
    Random,
}
impl Default for Method {
    fn default() -> Self {
        Self::Ring
    }
}

#[tracing::instrument]
pub async fn bootstrap(opts: Opts) -> Result<()> {
    match opts.method {
        Method::Ring => ring(opts.n, opts.total).await?,
        Method::Random => random(opts.n, opts.total).await?,
    }
    Ok(())
}

#[tracing::instrument]
async fn ring(n: usize, total: usize) -> Result<()> {
    let addrs = all_peer_addrs(total).await?;
    for peer in 0..total {
        for i in 0..n {
            let other = (peer + i + 1) % total;
            debug!(%peer, %other, "ring peer connection");
            if let Some(peer_addr) = &addrs.get(&other) {
                if let Err(err) = connect_peers(peer, peer_addr).await {
                    error!(%peer, %other, ?err, "failed to bootstrap peer");
                }
            }
        }
    }
    Ok(())
}
#[tracing::instrument]
async fn random(n: usize, total: usize) -> Result<()> {
    let mut rng = rand::thread_rng();
    let addrs = all_peer_addrs(total).await?;
    // Reuse a peers buffer for each loop
    let mut peers = vec![0; n];
    for peer in 0..total {
        // Randomly pick peers ensuring we do not pick duplicates
        // and do not pick the peer itself or peers that were unreachable.
        (0..total)
            .filter(|i| *i != peer && addrs.contains_key(i))
            .choose_multiple_fill(&mut rng, &mut peers);
        for other in &peers {
            debug!(%peer, %other, "random peer connection");
            if let Some(peer_addr) = &addrs.get(&other) {
                if let Err(err) = connect_peers(peer, peer_addr).await {
                    error!(%peer, %other, ?err, "failed to bootstrap peer");
                }
            }
        }
    }
    Ok(())
}
