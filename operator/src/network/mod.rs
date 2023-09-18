//! Network is k8s custom resource that defines a Ceramic network.
pub(crate) mod bootstrap;
pub(crate) mod cas;
pub(crate) mod ceramic;
pub(crate) mod controller;
pub(crate) mod datadog;
pub(crate) mod peers;
pub(crate) mod utils;

#[cfg(test)]
pub mod stub;

use crate::network::{cas::CasSpec, datadog::DataDogSpec};
// Expose Context for testing
#[cfg(test)]
pub use crate::utils::Context;

pub use bootstrap::BootstrapSpec;
pub use ceramic::{CeramicSpec, GoIpfsSpec, IpfsSpec, RustIpfsSpec};
pub use controller::run;

use keramik_common::peer_info::Peer;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Primary CRD for creating and managing a Ceramic network.
#[derive(CustomResource, Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "keramik.3box.io",
    version = "v1alpha1",
    kind = "Network",
    plural = "networks",
    status = "NetworkStatus",
    derive = "PartialEq"
)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSpec {
    /// Number of Ceramic peers
    pub replicas: i32,
    ///  Describes how new peers in the network should be bootstrapped.
    pub bootstrap: Option<BootstrapSpec>,
    /// Describes how each peer should behave.
    /// Multiple ceramic specs can be defined.
    /// Total replicas will be split across each ceramic spec according to relative weights.
    /// It is possible that if the weight is small enough compared to others that a single spec
    /// will be assigned zero replicas.
    pub ceramic: Vec<CeramicSpec>,
    /// Name of secret containing the private key used for signing anchor requests and generating
    /// the Admin DID.
    pub private_key_secret: Option<String>,
    /// Ceramic network type
    pub network_type: Option<String>,
    /// PubSub topic for Ceramic nodes to use
    pub pubsub_topic: Option<String>,
    /// Ethereum RPC URL for Ceramic nodes to use for verifying anchors
    pub eth_rpc_url: Option<String>,
    /// URL for Ceramic Anchor Service (CAS)
    pub cas_api_url: Option<String>,
    /// Describes how CAS should be deployed.
    pub cas: Option<CasSpec>,
    /// Descibes if/how datadog should be deployed.
    pub datadog: Option<DataDogSpec>,
}

/// Current status of the network.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    /// Number of Ceramic peers
    pub replicas: i32,
    ///  Describes how new peers in the network should be bootstrapped.
    pub ready_replicas: i32,
    /// K8s namespace this network is deployed in
    pub namespace: Option<String>,
    /// Information about each Ceramic peer
    pub peers: Vec<Peer>,
}
