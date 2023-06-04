use std::{cmp::min, collections::BTreeMap, path::PathBuf};

use anyhow::Result;
use clap::{Args, ValueEnum};
use keramik_common::peer_info::PeerInfo;
use rand::seq::IteratorRandom;
use tracing::{debug, error};

use crate::utils::{connect_peers, parse_peers_info};

/// Options to Bootstrap command
#[derive(Args, Debug)]
pub struct Opts {
    /// Bootstrap method to use.
    #[arg(long, value_enum, default_value_t, env = "BOOTSTRAP_METHOD")]
    method: Method,

    /// Number of peers to connect to.
    #[arg(long, env = "BOOTSTRAP_N")]
    n: usize,

    /// Path to file containing the list of peers.
    /// File should contian JSON encoding of Vec<PeerInfo>.
    #[arg(long, env = "BOOTSTRAP_PEERS_PATH")]
    peers: PathBuf,
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
    let peers = parse_peers_info(opts.peers).await?;
    match opts.method {
        Method::Ring => ring(opts.n, &peers).await?,
        Method::Random => random(opts.n, &peers).await?,
    }
    Ok(())
}

#[tracing::instrument(skip(peers), fields(peers.len = peers.len()))]
async fn ring(n: usize, peers: &BTreeMap<usize, PeerInfo>) -> Result<()> {
    for peer_info in peers.values() {
        // Connect to each peer in a ring.
        // Do not attempt to connect to more peers than exist.
        for i in 0..min(n, peers.len() - 1) {
            let idx = peer_info.index as usize;
            let other = (idx + i + 1) % peers.len();
            debug!(%idx, %other, "ring peer connection");
            if let Some(other_info) = peers.get(&other) {
                if let Err(err) = connect_peers(peer_info, other_info).await {
                    error!(
                        ?peer_info.index,
                        ?other_info.index,
                        ?err,
                        "failed to bootstrap ring peer"
                    );
                }
            }
        }
    }
    Ok(())
}
#[tracing::instrument(skip(peers), fields(peers.len = peers.len()))]
async fn random(n: usize, peers: &BTreeMap<usize, PeerInfo>) -> Result<()> {
    let mut rng = rand::thread_rng();
    // Reuse a peers buffer for each loop
    let mut other_peers = vec![0; min(n, peers.len() - 1)];
    for peer in peers.values() {
        // Randomly pick peers ensuring we do not pick duplicates
        // and do not pick the peer itself or peers that were unreachable.
        peers
            .keys()
            .copied()
            .filter(|other_idx| *other_idx != peer.index as usize)
            .choose_multiple_fill(&mut rng, &mut other_peers);
        for other in &other_peers {
            let other = peers.get(other).unwrap();
            debug!(?peer, ?other, "random peer connection");
            if let Err(err) = connect_peers(peer, other).await {
                error!(?peer, ?other, ?err, "failed to bootstrap random peer");
            }
        }
    }
    Ok(())
}
