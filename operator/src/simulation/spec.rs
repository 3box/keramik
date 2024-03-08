use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Primary CRD for creating and managing a Ceramic Simulation.
#[derive(CustomResource, Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "keramik.3box.io",
    version = "v1alpha1",
    kind = "Simulation",
    plural = "simulations",
    status = "SimulationStatus",
    derive = "PartialEq",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct SimulationSpec {
    /// Simulation runner scenario
    pub scenario: String,
    /// Name of the network to use.
    /// One of:
    ///   - mainnet:      Production network
    ///   - testnet-clay: Test network
    ///   - dev-unstable: Developement network
    ///   - local:        Local network, always uses 0 for the local id
    ///   - in-memory:    Singleton network in memory
    ///
    ///   Defaults to local
    pub network: Option<String>,
    /// Number of users
    pub users: u32,
    /// Time in minutes to run the simulation
    pub run_time: u32,
    /// Image for all jobs created by the simulation.
    pub image: Option<String>,
    /// Pull policy for image.
    pub image_pull_policy: Option<String>,
    /// Throttle requests (per second) for a simulation. Currently on a per-worker basis.
    pub throttle_requests: Option<usize>,
    /// Request target for the scenario to be a success. Scenarios can use this to
    /// validate throughput and correctness before returning. The exact definition is
    /// left to the scenario (requests per second, total requests, rps/node etc).
    pub success_request_target: Option<usize>,
    /// Enable dev mode for the simulation. This will remove resource limits that are not
    /// explicitly set in the simulation spec.
    pub(crate) dev_mode: Option<bool>,
}

impl Simulation {
    /// Whether the simulation is in dev mode, meaning resource limits are not applied unless specified.
    /// May have other implications in the future.
    pub fn dev_mode(&self) -> bool {
        self.spec.dev_mode.unwrap_or(false)
    }
}

/// Current status of a simulation.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulationStatus {
    /// Unique name for this simulation
    pub name: String,
    /// Unique value for this simulation.
    /// Used to enable determisitically psuedo-random values during any simulation logic.
    pub nonce: u32,
}
