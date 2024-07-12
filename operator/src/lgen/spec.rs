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
    status = "LoadGeneratorStatus",
    derive = "PartialEq",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct LoadGeneratorSpec {
    /// Load generator scenario
    pub scenario: String,
    /// Number of users
    pub users: u32,
    /// Time in minutes to run the load generator
    pub run_time: u32,
    /// Image for all jobs created by the load generator.
    pub image: Option<String>,
    /// Pull policy for image.
    pub image_pull_policy: Option<String>,
    /// Throttle requests (per second) for a load generator. Currently on a per-worker basis.
    pub throttle_requests: Option<usize>,
    /// Request target for the scenario to be a success.
    pub success_request_target: Option<usize>,
    /// Enable dev mode for the load generator.
    pub(crate) dev_mode: Option<bool>,
    /// Log level to use.
    pub(crate) log_level: Option<String>,
    /// Anchor wait time in seconds, use with ceramic-anchoring-benchmark scenario
    pub anchor_wait_time: Option<u32>,
    /// Network type to use for the load generator, use with cas-benchmark scenario
    pub cas_network: Option<String>,
    /// Controller DID for the load generator, use with cas-benchmark scenario
    pub cas_controller: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
pub struct LoadGeneratorStatus {
    pub name: String,
    pub nonce: u32,
}