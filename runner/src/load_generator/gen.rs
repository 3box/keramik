use std::collections::HashMap;
use std::path::PathBuf;

use crate::load_generator::utils::{
    CeramicConfig, CeramicDidType, CeramicScenarioParameters, StableLoadUser,
};
use crate::utils::parse_peers_info;
use crate::CommandResult;
use anyhow::Result;
use ceramic_core::StreamId;
use clap::Args;
use keramik_common::peer_info::Peer;
use tokio::time::{Duration, Instant};

// TODO : Use this to envoke a particular scenario, currently we only have one
// so this is unused
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum LoadGenScenarios {
    CreateModelInstancesSynced,
}

impl std::str::FromStr for LoadGenScenarios {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CreateModelInstancesSynced" => Ok(LoadGenScenarios::CreateModelInstancesSynced),
            _ => Err(format!("Invalid scenario: {}", s)),
        }
    }
}

/// Options to Simulate command
#[derive(Args, Debug, Clone)]
pub struct LoadGenOpts {
    /// Simulation scenario to run.
    #[arg(long, env = "GENERATOR_SCENARIO")]
    scenario: LoadGenScenarios,

    /// Path to file containing the list of peers.
    /// File should contian JSON encoding of Vec<Peer>.
    #[arg(long, env = "GENERATOR_PEERS_PATH")]
    peers: PathBuf,

    /// Implmentation details: A task corresponds to a tokio task responsible
    /// for making requests. They should have low memory overhead, so you can
    /// create many tasks and then use `throttle_requests_rate` to constrain the overall
    /// throughput on the node (specifically the HTTP requests made).
    #[arg(long, default_value_t = 25, env = "GENERATOR_TASKS")]
    tasks: usize,

    /// Duration of the simulation in hours
    #[arg(long, env = "GENERATOR_RUN_TIME", default_value = "5h")]
    run_time: String,

    /// Unique value per test run to ensure uniqueness across different generator runs
    #[arg(long, env = "GENERATOR_NONCE")]
    nonce: u64,

    /// Option to throttle requests (per second) for load control
    #[arg(long, env = "GENERATOR_THROTTLE_REQUESTS_RATE")]
    throttle_requests_rate: Option<usize>,
}

//TODO : Use week long simulation scenario and separate out the logic which is ties to a particular scenario
// TODO : This specific behavior is for createModelInstancesSynced scenario
pub async fn simulate_load(opts: LoadGenOpts) -> Result<CommandResult> {
    let state = WeekLongSimulationState::try_from_opts(opts).await?;

    // Create two configs to simulate two independent nodes, each having it's own ceramic client
    let config_1 = state.initialize_config().await?;
    let config_2 = state.initialize_config().await?;

    let peer_addr_1 = state.peers[0]
        .ceramic_addr()
        .expect("Peer does not have a ceramic address");
    let peer_addr_2 = state.peers[1]
        .ceramic_addr()
        .expect("Peer does not have a ceramic address");

    // Create two users to simulate two independent nodes
    let stable_load_user_1 =
        StableLoadUser::setup_stability_test(config_1.admin_cli, Some(peer_addr_1.to_string()))
            .await;
    let stable_load_user_2 =
        StableLoadUser::setup_stability_test(config_2.admin_cli, Some(peer_addr_2.to_string()))
            .await;

    // Generate a model for the users to create
    let model = stable_load_user_1
        .ceramic_utils
        .generate_random_model()
        .await?;

    // Index the model on the second node
    stable_load_user_2.ceramic_utils.index_model(&model).await?;

    let run_time: u64 = state
        .run_time
        .parse()
        .expect("Failed to parse run_time as u64");

    println!("Model: {:?}", model);
    let model_instance_creation_result =
        create_model_instances_continuously(stable_load_user_1, model, run_time, state.tasks).await;
    println!(
        "Model instance creation result: {:?}",
        model_instance_creation_result
    );

    Ok(CommandResult::Success)
}

/**
 * Create model instances continuously
 *
 * @param stable_load_user The user to create the model instances
 * @param model The model schema to create model instances from
 * @param duration_in_hours The duration to run the simulation in hours
 * @return The result of the simulation
 */
pub async fn create_model_instances_continuously(
    stable_load_user: StableLoadUser,
    model: StreamId,
    duration_in_hours: u64,
    tasks_count: usize,
) -> Result<()> {
    let start_time = Instant::now();

    let duration = Duration::from_secs(duration_in_hours * 60 * 60);
    let mut count = 0;
    let mut error_map: HashMap<String, u64> = HashMap::new();
    // TODO : Make the rps configurable
    // TODO : Make the channel size configurable
    // TODO : Make the number of tasks configurable : tasks are currently 100 -
    // increasing tasks can help increase throughput
    let (tx, mut rx) = tokio::sync::mpsc::channel(10000);
    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..tasks_count {
        let user_clone = stable_load_user.clone();
        let model = model.clone();
        let tx = tx.clone();
        tasks.spawn(async move {
            loop {
                if start_time.elapsed() > duration {
                    println!("loop {i} Duration expired");
                    break;
                }
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    user_clone.ceramic_utils.create_random_mid(&model),
                )
                .await
                {
                    Ok(Ok(mid)) => match tx.send(Ok(mid.to_string())).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Failed to send MID: {}", e);
                        }
                    },
                    Ok(Err(e)) => match tx.send(Err(e.to_string())).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Failed to send error: {}", e);
                        }
                    },
                    Err(e) => match tx.send(Err(e.to_string())).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Failed to send error: {}", e);
                        }
                    },
                }
            }
        });
    }
    // Drop the tx sender, since the exit condition below requires the senders to be dropped for termination
    drop(tx);
    loop {
        let mut mid_vec: Vec<Result<String, String>> = Vec::with_capacity(10);
        if rx.recv_many(&mut mid_vec, 10).await > 0 {
            for mid in mid_vec {
                match mid {
                    Ok(_) => {
                        count += 1;
                    }
                    Err(err) => {
                        *error_map.entry(err).or_insert(0) += 1;
                    }
                }
            }
        }
        // Add a small buffer to the duration to account for the time it takes to send the MIDs
        if start_time.elapsed() > duration + Duration::from_secs(5) {
            tasks.abort_all();
            break;
        }
    }
    // After the loop, print the error map
    // TODO : Add observability to this, report these errors/counts
    println!("Error counts:");
    for (error, count) in &error_map {
        println!("Error: {}, Count: {}", error, count);
    }
    println!("Created {} MIDs in {} hours", count, duration_in_hours);
    println!(
        "Failed to create {} MIDs in {} hours",
        error_map.values().sum::<u64>(),
        duration_in_hours
    );
    Ok(())
}

struct WeekLongSimulationState {
    pub peers: Vec<Peer>,
    pub run_time: String,
    pub tasks: usize,
}

impl WeekLongSimulationState {
    /**
     * Try to create a new instance of the WeekLongSimulationState from the given options
     *
     * @param opts The options to use
     * @return The created instance
     */
    async fn try_from_opts(opts: LoadGenOpts) -> Result<Self> {
        Ok(Self {
            peers: parse_peers_info(opts.peers.clone()).await?,
            run_time: opts.run_time,
            tasks: opts.tasks,
        })
    }

    /**
     * Initialize the configuration for the WeekLongSimulationState
     *
     * @return The created configuration
     */
    async fn initialize_config(&self) -> Result<CeramicConfig> {
        // Create a CeramicScenarioParameters instance with default values
        let params = CeramicScenarioParameters {
            did_type: CeramicDidType::EnvInjected,
        };

        CeramicConfig::initialize_config(params).await
    }
}
