use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Primary CRD for creating and managing a Load Generator.
#[derive(CustomResource, Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "keramik.3box.io",
    version = "v1alpha1",
    kind = "LoadGenerator",
    plural = "loadgenerators",
    status = "LoadGeneratorState",
    derive = "PartialEq",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct LoadGeneratorSpec {
    /// Load generator scenario
    pub scenario: String,
    /// Time in minutes to run the load generator
    pub run_time: u32,
    /// Image for all jobs created by the load generator.
    pub image: Option<String>,
    /// Pull policy for image.
    pub image_pull_policy: Option<String>,
    /// Throttle requests (per second) for a load generator. Currently on a per-worker basis.
    pub throttle_requests: Option<usize>,
    /// Number of tasks to run in parallel
    pub tasks: usize,
}

/// Status of the load generator.
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
pub struct LoadGeneratorState {
    /// Name of the load generator.
    pub name: String,
    /// Nonce for the load generator.
    pub nonce: u32,
}
