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
    /// Throttle requests (per second) for a simulation
    pub throttle_requests: Option<usize>,
}

/// Current status of a simulation.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulationStatus {
    /// Unique value for this simulation.
    /// Used to enable determisitically psuedo-random values during any simulation logic.
    pub nonce: u32,
}
