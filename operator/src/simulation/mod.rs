//! Simulation is a k8s custom resource that defines a Ceramic simulation.
pub(crate) mod worker;
pub(crate) mod manager;
pub(crate) mod controller;

pub use controller::run;

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rand::random;

/// Primary CRD for creating and managing a Ceramic Simulation.
#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "keramik.3box.io",
    version = "v1alpha1",
    kind = "Simulation",
    plural = "simulations",
    status = "SimulationStatus",
    derive = "PartialEq"
)]
pub struct SimulationSpec {
  /// Selector of the cluster to run against.
  /// This selector must select a Network resource.
  pub selector: String,
  /// Simulation runner scenario
  pub scenario: String,
  /// Number of users
  pub users: u32,
  /// Time to run simulation
  pub run_time: u32,
}

/// Defines the current operating mode of the simulation
// #[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
// pub enum SimulationPhase {
//   /// Configure each node with its interested datasets
//   Bootstrap,
//   /// Writing data into the network
//   Write,
//   /// Reading and validating data from the network
//   Read,
//   /// Taking a snapshot of metrics from the network
//   Snapshot,
//   /// Remove interested datasets from each node
//   Cleanup,
//   /// Simulation is complete, it finished at the specified time
//   Complete(Time),
//   /// Simulation failed, and the time of failure
//   Failed(Time)
// }

/// Current status of the network.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimulationStatus {
  //  TODO ENUM states
  phase: String,
  nonce: u32, 
}

impl Default for SimulationStatus {
  fn default() -> SimulationStatus {
    SimulationStatus {
        phase: "initialize".to_owned(),
        nonce: random::<u32>(), 
      }
  }
}
