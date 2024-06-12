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
    /// Log level to use. On the manager writes to stdout, and on the workers will write
    /// files for the simulation. The files may not be flushed until the simulation is complete
    /// which consumes RAM, and will disappear if there is no persistent volume when the pod exits.
    /// Valid values: 'warn', 'info', 'debug', 'trace'. Defaults to None meaning no logging beyond RUST_LOG.
    pub(crate) log_level: Option<String>,
    /// Anchor wait time in seconds
    pub anchor_wait_time: Option<u32>,
    /// Network type to use for the simulation.
    pub cas_network: Option<String>,
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
