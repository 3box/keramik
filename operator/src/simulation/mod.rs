//! Simulation is a k8s custom resource that defines a Ceramic simulation.
pub(crate) mod controller;
pub(crate) mod manager;
#[cfg(test)]
pub mod stub;
pub(crate) mod worker;
pub(crate) mod datadog;

pub use controller::run;

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rand::random;

use crate::simulation::datadog::DataDogSpec;

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
    /// Image for all jobs created by the simulation.
    pub image: Option<String>,
    /// Pull policy for image.
    pub image_pull_policy: Option<String>,
    /// DataDog telemetry configuration
    pub datadog: Option<DataDogSpec>,
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

/// Configuration for job images.
#[derive(Clone, Debug)]
pub struct JobImageConfig {
    /// Image for all jobs created by the simulation.
    pub image: String,
    /// Pull policy for image.
    pub image_pull_policy: String,
}

impl Default for JobImageConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/keramik-runner:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
        }
    }
}

impl From<&SimulationSpec> for JobImageConfig {
    fn from(value: &SimulationSpec) -> Self {
        let default = Self::default();
        Self {
            image: value.image.to_owned().unwrap_or(default.image),
            image_pull_policy: value
                .image_pull_policy
                .to_owned()
                .unwrap_or(default.image_pull_policy),
        }
    }
}
