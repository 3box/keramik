use anyhow::Result;
use clap::{Args, ValueEnum};
use rand::seq::IteratorRandom;
use tracing::debug;

use crate::utils::{connect_peers, NodeId};

/// Options to Bootstrap command
#[derive(Args, Debug)]
pub struct Opts {
    /// Bootstrap method to use.
    #[arg(long, value_enum, default_value_t, env = "BOOTSTRAP_METHOD")]
    method: Method,

    /// Node to be bootstrapped.
    #[arg(long, env = "BOOTSTRAP_NODE")]
    node: String,

    /// Number of peers to connecto to.
    #[arg(long, env = "BOOTSTRAP_N")]
    n: usize,

    /// Total number of peers in the network
    #[arg(long, env = "BOOTSTRAP_TOTAL")]
    total: usize,
}

#[derive(Clone, Debug, ValueEnum)]
enum Method {
    // Connects to next N peers
    Ring,
    Random,
}
impl Default for Method {
    fn default() -> Self {
        Self::Ring
    }
}

pub async fn run(opts: Opts) -> Result<()> {
    debug!(method = ?opts.method, "bootstrap");
    match opts.method {
        Method::Ring => ring(opts.node.parse()?, opts.n, opts.total).await?,
        Method::Random => random(opts.node.parse()?, opts.n, opts.total).await?,
    }
    Ok(())
}

async fn ring(node: NodeId, n: usize, total: usize) -> Result<()> {
    for i in 0..n {
        let peer = (node + i + 1) % total;
        connect_peers(node, peer).await?;
    }
    Ok(())
}
async fn random(node: NodeId, n: usize, total: usize) -> Result<()> {
    let mut rng = rand::thread_rng();
    // Randomly pick peers ensuring we do not pick duplicates
    // and do not pick the node itself.
    for peer in (0..total)
        .filter(|i| *i != node)
        .choose_multiple(&mut rng, n)
    {
        connect_peers(node, peer).await?;
    }
    Ok(())
}
