//! Network is k8s custom resource that defines a Ceramic network.
pub(crate) mod bootstrap;
pub(crate) mod cas;
pub(crate) mod ceramic;
pub(crate) mod controller;
pub(crate) mod peers;
pub(crate) mod utils;

#[cfg(test)]
pub mod stub;
// Expose Context for testing
#[cfg(test)]
pub use controller::Context;

pub use bootstrap::BootstrapSpec;
pub use ceramic::CeramicSpec;
pub use controller::run;

use keramik_common::peer_info::PeerInfo;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Primary CRD for creating and managing a Ceramic network.
#[derive(CustomResource, Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "keramik.3box.io",
    version = "v1",
    kind = "Network",
    plural = "networks",
    status = "NetworkStatus",
    derive = "PartialEq"
)]
pub struct NetworkSpec {
    /// Number of Ceramic peers
    pub replicas: i32,
    ///  Describes how new peers in the network should be bootstrapped.
    pub bootstrap: Option<BootstrapSpec>,
    /// Describes how each peer should behave.
    pub ceramic: Option<CeramicSpec>,
}

/// Current status of the network.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    replicas: i32,
    ready_replicas: i32,
    namespace: Option<String>,
    // Information about each Ceramic peer
    peers: Vec<PeerInfo>,
}
