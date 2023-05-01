use anyhow::Result;
use clap::{Args, ValueEnum};
use goose::{config::GooseConfiguration, GooseAttack};
use tracing::error;

use crate::{
    scenario::ipfs_block_fetch,
    utils::{PeerId, PeerRpcAddr},
};

/// Options to Simulate command
#[derive(Args, Debug)]
pub struct Opts {
    /// Simulation scenario to run.
    #[arg(long, value_enum, env = "SIMULATE_SCENARIO")]
    scenario: Scenario,

    /// Id of the peer to target.
    #[arg(long, env = "SIMULATE_MANAGER")]
    manager: bool,

    /// Id of the peer to target.
    #[arg(long, env = "SIMULATE_TARGET_PEER")]
    target_peer: usize,

    /// Total number of peers in the network
    #[arg(long, env = "SIMULATE_TOTAL_PEERS")]
    total_peers: usize,

    /// Number of users to simulate
    #[arg(long, default_value_t = 100, env = "SIMULATE_USERS")]
    users: usize,

    /// Number of iterations of the scenario each user should perform.
    #[arg(long, default_value_t = 10, env = "SIMULATE_ITERATIONS")]
    iterations: usize,

    /// Unique value per test run to ensure uniqueness across different test runs.
    /// All workers and manager must be given the same nonce.
    #[arg(long, env = "SIMULATE_NONCE")]
    nonce: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Topology {
    pub target_worker: usize,
    pub total_workers: usize,
    pub nonce: u64,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum Scenario {
    /// Queries the Id of the IPFS peers.
    IpfsRpc,
}

#[tracing::instrument]
pub async fn simulate(opts: Opts) -> Result<()> {
    // We assume exactly one worker per peer.
    // This allows us to be deterministic in how each user operates.
    let topo = Topology {
        target_worker: opts.target_peer,
        total_workers: opts.total_peers,
        nonce: opts.nonce,
    };

    let scenario = match opts.scenario {
        Scenario::IpfsRpc => ipfs_block_fetch::scenario(topo)?,
    };
    let config = if opts.manager {
        manager_config(opts.total_peers, opts.users, opts.iterations)
    } else {
        worker_config(opts.target_peer)
    };
    // TODO use the metrics
    let _metrics = match GooseAttack::initialize_with_config(config)?
        .register_scenario(scenario)
        .execute()
        .await
    {
        Ok(m) => m,
        Err(e) => {
            error!("{:#?}", e);
            return Err(e.into());
        }
    };
    Ok(())
}

fn manager_config(total: usize, users: usize, iterations: usize) -> GooseConfiguration {
    let mut config = GooseConfiguration::default();
    config.users = Some(users);
    config.iterations = iterations;
    // manager requires a host to be set even if its not doing any simulations.
    config.host = PeerRpcAddr::from(0).as_string();
    config.manager = true;
    config.manager_bind_port = 5115;
    config.expect_workers = Some(total);
    config
}
fn worker_config(target_peer: PeerId) -> GooseConfiguration {
    let mut config = GooseConfiguration::default();
    config.worker = true;
    config.host = PeerRpcAddr::from(target_peer).as_string();
    config.manager_host = "manager.goose.keramik-0.svc.cluster.local".to_string();
    config.manager_port = 5115;
    config
}
