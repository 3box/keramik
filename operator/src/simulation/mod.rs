//! Simulation is a k8s custom resource that defines a Ceramic simulation.
pub(crate) mod controller;
pub(crate) mod manager;
#[cfg(test)]
pub mod stub;
pub(crate) mod worker;

pub use controller::run;

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rand::random;

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
pub struct SimulationSpec {
    /// Simulation runner scenario
    pub scenario: String,
    /// Number of users
    pub users: u32,
    /// Time to run simulation
    pub run_time: u32,
}

/// Current status of a simulation.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulationStatus {
    nonce: u32,
}

impl Default for SimulationStatus {
    fn default() -> SimulationStatus {
        SimulationStatus {
            nonce: random::<u32>(),
        }
    }
}
