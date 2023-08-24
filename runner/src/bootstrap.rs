use std::{cmp::min, path::PathBuf};

use anyhow::Result;
use clap::{Args, ValueEnum};
use keramik_common::peer_info::Peer;
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
    /// File should contian JSON encoding of Vec<Peer>.
    #[arg(long, env = "BOOTSTRAP_PEERS_PATH")]
    peers: PathBuf,
}

#[derive(Clone, Debug, ValueEnum)]
enum Method {
    /// Connects to next N peers
    Ring,
    /// Connects to N peers at random.
    Random,
    /// Connects each peer to the first N peers.
    Sentinel,
}
impl Default for Method {
    fn default() -> Self {
        Self::Ring
    }
}

#[tracing::instrument]
pub async fn bootstrap(opts: Opts) -> Result<()> {
    let peers = parse_peers_info(opts.peers).await?;
    // Bootstrap peers according to the given method.
    // Methods should not assume that peer indexes are consecutive nor that they start at zero.
    match opts.method {
        Method::Ring => ring(opts.n, &peers).await?,
        Method::Random => random(opts.n, &peers).await?,
        Method::Sentinel => sentinel(opts.n, &peers).await?,
    }
    Ok(())
}

#[tracing::instrument(skip(peers), fields(peers.len = peers.len()))]
async fn ring(n: usize, peers: &[Peer]) -> Result<()> {
    for (i, peer) in peers.iter().enumerate() {
        // Connect to each peer in a ring.
        for other in peers
            .iter()
            .enumerate()
            .cycle()
            .skip_while(|(j, _)| j <= &i)
            .take(n)
            .map(|(_, peer)| peer)
        {
            debug!(peer = peer.id(), other = other.id(), "ring peer connection");
            if let Err(err) = connect_peers(peer, other).await {
                error!(
                    peer = peer.id(),
                    other = other.id(),
                    ?err,
                    "failed to bootstrap ring peer"
                );
            }
        }
    }
    Ok(())
}
#[tracing::instrument(skip(peers), fields(peers.len = peers.len()))]
async fn random(n: usize, peers: &[Peer]) -> Result<()> {
    let mut rng = rand::thread_rng();
    // Reuse a peers buffer for each loop
    let mut other_peers = vec![0; min(n, peers.len() - 1)];
    for (i, peer) in peers.iter().enumerate() {
        // Randomly pick peers ensuring we do not pick duplicates
        // and do not pick the peer itself or peers that were unreachable.
        peers
            .iter()
            .enumerate()
            .map(|(other_idx, _)| other_idx)
            // Skip self
            .filter(|other_idx| other_idx != &i)
            .choose_multiple_fill(&mut rng, &mut other_peers);
        for other_idx in &other_peers {
            let other = peers
                .get(*other_idx)
                .expect("other_idx should always exist");
            debug!(?peer, ?other, "random peer connection");
            if let Err(err) = connect_peers(peer, other).await {
                error!(?peer, ?other, ?err, "failed to bootstrap random peer");
            }
        }
    }
    Ok(())
}

#[tracing::instrument(skip(peers), fields(peers.len = peers.len()))]
async fn sentinel(n: usize, peers: &[Peer]) -> Result<()> {
    for (i, peer) in peers.iter().enumerate() {
        // Connect to each peer to the first n peers.
        for sentinel in peers
            .iter()
            .enumerate()
            // Skip connecting to self if we are a sentinel peer
            .filter(|(idx, _)| idx != &i)
            .take(n)
            .map(|(_, peer)| peer)
        {
            debug!(
                peer = peer.id(),
                sentinel = sentinel.id(),
                "sentinel peer connection"
            );
            if let Err(err) = connect_peers(peer, sentinel).await {
                error!(
                    peer = peer.id(),
                    sentinel = sentinel.id(),
                    ?err,
                    "failed to bootstrap sentinel peer"
                );
            }
        }
    }
    Ok(())
}
