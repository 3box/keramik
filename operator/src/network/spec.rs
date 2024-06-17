//! Place all spec types into a single module so they can be used as a lightweight dependency
use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
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
    pub ceramic: Option<Vec<CeramicSpec>>,
    /// Name of secret containing the private key used for signing anchor requests and generating
    /// the Admin DID.
    pub private_key_secret: Option<String>,
    /// Ceramic network type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_type: Option<NetworkType>,
    /// Ethereum RPC URL for Ceramic nodes to use for verifying anchors
    pub eth_rpc_url: Option<String>,
    /// URL for Ceramic Anchor Service (CAS)
    pub cas_api_url: Option<String>,
    /// Describes how CAS should be deployed.
    pub cas: Option<CasSpec>,
    /// Descibes if/how datadog should be deployed.
    pub datadog: Option<DataDogSpec>,
    /// The number of seconds this network should live.
    /// If unset the network lives forever.
    pub ttl_seconds: Option<u64>,
    /// Enable dev mode for the network. This will remove resource requests/limits that are not
    /// explicitly overridden by the spec. This allows deploying the network on a smaller machine,
    /// as well as running every container with unlimited resources.
    pub dev_mode: Option<bool>,
    /// Enable debug mode for the network. This will add capabilities to containers to enable
    /// process tracing and other debug related features.
    /// Generally do NOT run debug_mode on production infrastructure as it has weaker security
    /// properties.
    pub debug_mode: Option<bool>,
    /// Enable monitoring resources to be deployed into the network.
    pub monitoring: Option<MonitoringSpec>,
    /// A list of node selector terms. These node select terms will be applied to all pods in the network.
    /// As such you must ensure that the selected nodes have the capacity to handle the entire
    /// network workload.
    pub node_selector_terms: Option<Vec<k8s_openapi::api::core::v1::NodeSelectorTerm>>,
}

/// Local network ID.
/// We can use a hard coded value since nodes from other networks should not be
/// able to connect.
pub const NETWORK_LOCAL_ID: usize = 0;

/// NetworkType is the discrete set of networks
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    /// Mainnet
    Mainnet,
    /// Testnet Clay
    #[serde(rename = "testnet-clay")]
    TestnetClay,
    /// DevUnstable
    #[serde(rename = "dev-unstable")]
    DevUnstable,
    /// Local (default)
    #[default]
    Local,
    /// InMemory, cannot participate with other nodes
    InMemory,
}

impl NetworkType {
    /// Returns a unique name for the network
    pub fn name(&self) -> &str {
        match self {
            NetworkType::Mainnet => "mainnet",
            NetworkType::TestnetClay => "testnet-clay",
            NetworkType::DevUnstable => "dev-unstable",
            NetworkType::Local => "local",
            NetworkType::InMemory => "inmemory",
        }
    }
    /// Returns a unique name for a pubsub topic for the network
    pub fn topic(&self) -> String {
        match self {
            NetworkType::Mainnet => "/ceramic/mainnet".to_string(),
            NetworkType::TestnetClay => "/ceramic/testnet-clay".to_string(),
            NetworkType::DevUnstable => "/ceramic/dev-unstable".to_string(),
            NetworkType::Local => format!("/ceramic/local-{NETWORK_LOCAL_ID}"),
            NetworkType::InMemory => "/ceramic/inmemory".to_string(),
        }
    }
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
    /// Time when the network will expire and be deleted.
    /// If unset the network lives forever.
    pub expiration_time: Option<k8s_openapi::apimachinery::pkg::apis::meta::v1::Time>,
}

/// BootstrapSpec defines how the network bootstrap process should proceed.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSpec {
    /// When true bootstrap job will run, defaults to true.
    pub enabled: Option<bool>,
    /// Image of the runner for the bootstrap job.
    pub image: Option<String>,
    /// Image pull policy for the bootstrap job.
    pub image_pull_policy: Option<String>,
    /// Bootstrap method. Defaults to ring.
    pub method: Option<String>,
    /// Number of nodes to connect to each peer.
    pub n: Option<i32>,
}

/// Describes how a Ceramic peer should behave.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CeramicSpec {
    /// Relative weight of the spec compared to others.
    pub weight: Option<i32>,
    /// Name of a config map with a ceramic-init.sh script that runs as an initialization step.
    pub init_config_map: Option<String>,
    /// Name of the image to initialize the ceramic container.
    pub init_image_name: Option<String>,
    /// Image of the ceramic container.
    pub image: Option<String>,
    /// Pull policy for the ceramic container image.
    pub image_pull_policy: Option<String>,
    /// Configuration of the IPFS container
    pub ipfs: Option<IpfsSpec>,
    /// Resource limits for ceramic nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
    /// Storage configuration for the ceramic container.
    pub storage: Option<PersistentStorageSpec>,
    /// Resource limits for postgres container in ceramic nodes, applies to both requests and limits.
    pub postgres_resource_limits: Option<ResourceLimitsSpec>,
    /// Storage configuration for the postgres container.
    pub postgres_storage: Option<PersistentStorageSpec>,
    /// Extra env values to pass to the image.
    /// CAUTION: Any env vars specified in this set will override any predefined values.
    pub env: Option<BTreeMap<String, String>>,
}

/// Describes how the IPFS node for a peer should behave.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
// Clippy seems be warning for a false positive.
// It's saying 0 and at least 216, when it should be at least 248 for RustIpfsSpec.
pub enum IpfsSpec {
    /// Rust IPFS specification
    Rust(RustIpfsSpec),
    /// Go IPFS specification
    Go(GoIpfsSpec),
}

/// Describes how the Rust IPFS node for a peer should behave.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RustIpfsSpec {
    /// Name of image to use
    pub image: Option<String>,
    /// Image pull policy for the image
    pub image_pull_policy: Option<String>,
    /// Resource limits for ipfs nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
    /// Persistent storage configuration
    pub storage: Option<PersistentStorageSpec>,
    /// Name of the storage class for the PVC of the IPFS container
    pub storage_class: Option<String>,
    /// Value of the RUST_LOG env var.
    pub rust_log: Option<String>,
    /// Extra env values to pass to the image.
    /// CAUTION: Any env vars specified in this set will override any predefined values.
    pub env: Option<BTreeMap<String, String>>,
    /// Migration command that should run before a node comes up
    pub migration_cmd: Option<String>,
}

/// Describes how the Go IPFS node for a peer should behave.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoIpfsSpec {
    /// Name of image to use
    pub image: Option<String>,
    /// Image pull policy for the image
    pub image_pull_policy: Option<String>,
    /// Resource limits for ipfs nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
    /// Persistent storage configuration
    pub storage: Option<PersistentStorageSpec>,
    /// Name of the storage class for the PVC of the IPFS container
    pub storage_class: Option<String>,
    /// List of ipfs commands to run during initialization.
    pub commands: Option<Vec<String>>,
}

/// Defines details about how CAS is deployed
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CasSpec {
    /// Image of CAS
    pub image: Option<String>,
    /// Image pull policy for the bootstrap job.
    pub image_pull_policy: Option<String>,
    /// Configuration of the IPFS container
    pub ipfs: Option<IpfsSpec>,
    /// Resource limits for the CAS pod, applies to both requests and limits.
    pub cas_resource_limits: Option<ResourceLimitsSpec>,
    /// CAS storage configuration
    pub cas_storage: Option<PersistentStorageSpec>,
    /// Resource limits for the CAS IPFS pod, applies to both requests and limits.
    pub ipfs_resource_limits: Option<ResourceLimitsSpec>,
    /// IPFS container storage configuration
    pub ipfs_storage: Option<PersistentStorageSpec>,
    /// Resource limits for the Ganache pod, applies to both requests and limits.
    pub ganache_resource_limits: Option<ResourceLimitsSpec>,
    /// Ganache container storage configuration
    pub ganache_storage: Option<PersistentStorageSpec>,
    /// Resource limits for the CAS Postgres pod, applies to both requests and limits.
    pub postgres_resource_limits: Option<ResourceLimitsSpec>,
    /// Postgres container container storage configuration
    pub postgres_storage: Option<PersistentStorageSpec>,
    /// Resource limits for the LocalStack pod, applies to both requests and limits.
    pub localstack_resource_limits: Option<ResourceLimitsSpec>,
    /// Localstack container storage configuration
    pub localstack_storage: Option<PersistentStorageSpec>,
    /// Configuration for the CAS API
    pub api: Option<CasApiSpec>,
}

/// Describes how the CAS API is configured.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CasApiSpec {
    /// Extra env values to pass to the image.
    /// CAUTION: Any env vars specified in this set will override any predefined values.
    pub env: Option<BTreeMap<String, String>>,
}

/// Describes if and how to configure datadog telemetry
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataDogSpec {
    /// When true datadog telemetry will be collected.
    pub enabled: Option<bool>,
    /// Version of the DataDog agent.
    pub version: Option<String>,
    /// When true profiles will be collected.
    pub profiling_enabled: Option<bool>,
}

/// Describes the resources limits and requests for a pod
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimitsSpec {
    /// Cpu resource limit
    pub cpu: Option<Quantity>,
    /// Memory resource limit
    pub memory: Option<Quantity>,
    /// Ephemeral storage resource limit
    pub storage: Option<Quantity>,
}

/// Describes the resources limits and requests for a pod
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersistentStorageSpec {
    /// Size of the persistent disk to request
    pub size: Option<Quantity>,
    /// Name of the storage class for the PVC of the container
    pub class: Option<String>,
}

/// Describes how monitoring resources are deployed for the network
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSpec {
    /// Deploy monitoring resources into the network namespace directly
    pub namespaced: Option<bool>,
    /// Deploy pod monitors
    pub pod_monitor: Option<PodMonitorSpec>,
}

/// Describes the pod monitor configuration
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodMonitorSpec {
    /// Whether pod monitoring is enabled
    pub enabled: Option<bool>,
    /// Labels transferred from the pod onto the ingested metrics
    pub pod_target_labels: Option<Vec<String>>,
}

/// CRD for pod monitors.
#[derive(CustomResource, Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "monitoring.coreos.com",
    version = "v1",
    kind = "PodMonitor",
    plural = "podmonitors",
    namespaced,
    derive = "PartialEq"
)]
#[serde(rename_all = "camelCase")]
pub struct PodMonitorCrd {
    /// List of endpoints that are part of this monitor
    pub pod_metrics_endpoints: Vec<PodMetricsEndpointSpec>,
    /// Labels transferred from the pod onto the ingested metrics
    pub pod_target_labels: Option<Vec<String>>,
    /// Label selector to select pods
    pub selector: Option<SelectorSpec>,
}

/// Describes the pod monitor endpoint
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodMetricsEndpointSpec {
    /// Monitoring interval
    pub interval: Option<String>,
    /// Monitoring path
    pub path: Option<String>,
    /// Monitoring port
    pub target_port: Option<i32>,
}

/// Describes the selector for the pod monitor
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectorSpec {
    /// The selector match labels
    pub match_labels: BTreeMap<String, String>,
}
