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
    /// Monitoring resources for a simulation
    pub monitoring: Option<MonitoringSpec>,

}
/// MonitoringSpec defines how Simulation will be monitored.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSpec {
    /// Monitoring in the namespace
    pub local: Option<LocalMonitoringSpec>,
}

/// LocalMonitoringSpec defines monitoring in the namespace.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalMonitoringSpec {
    /// Deploy monitoring to the namespace
    pub enabled: bool,
}

/// Current status of a simulation.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulationStatus {
    /// Unique value for this simulation.
    /// Used to enable determisitically psuedo-random values during any simulation logic.
    pub nonce: u32,
}
