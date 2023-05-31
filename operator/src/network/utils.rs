use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use keramik_common::peer_info::{IpfsPeerInfo, PeerIdx};
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unimock::unimock;

use crate::network::controller::{
    CERAMIC_SERVICE_API_PORT, CERAMIC_SERVICE_IPFS_PORT, CERAMIC_SERVICE_NAME,
    CERAMIC_STATEFUL_SET_NAME,
};

#[derive(serde::Deserialize)]
struct ErrorResponse {
    #[serde(rename = "Message")]
    message: String,
}

/// Define the behavior we consume from the IPFS RPC API.
#[unimock(api=IpfsRpcClientMock)]
#[async_trait]
pub trait IpfsRpcClient {
    async fn peer_info(&self, index: PeerIdx, ipfs_rpc_addr: &str) -> Result<IpfsPeerInfo>;
    async fn peer_status(&self, ipfs_rpc_addr: &str) -> Result<PeerStatus>;
}

/// Status of the current peer
#[derive(Debug)]
pub struct PeerStatus {
    /// Number of connected peers
    pub connected_peers: i32,
}

pub struct HttpRpcClient;

/// Determine the IPFS RPC address of a Ceramic peer
pub fn ceramic_peer_ipfs_rpc_addr(ns: &str, peer: PeerIdx) -> String {
    format!("http://{CERAMIC_STATEFUL_SET_NAME}-{peer}.{CERAMIC_SERVICE_NAME}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_IPFS_PORT}")
}
// Determine the Ceramic addres of a Ceramic peer
pub fn ceramic_addr(ns: &str, peer: PeerIdx) -> String {
    format!("http://{CERAMIC_STATEFUL_SET_NAME}-{peer}.{CERAMIC_SERVICE_NAME}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_API_PORT}")
}

#[async_trait]
impl IpfsRpcClient for HttpRpcClient {
    async fn peer_info(&self, index: PeerIdx, ipfs_rpc_addr: &str) -> Result<IpfsPeerInfo> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/v0/id", ipfs_rpc_addr))
            .send()
            .await?;
        if !resp.status().is_success() {
            let data: ErrorResponse = resp.json().await?;
            bail!("peer id failed: {}", data.message)
        }

        #[derive(serde::Deserialize)]
        struct Response {
            #[serde(rename = "ID")]
            id: String,
            #[serde(rename = "Addresses")]
            addresses: Vec<String>,
        }
        let data: Response = resp.json().await?;

        let p2p_proto = Protocol::P2p(Multihash::from_bytes(
            &multibase::Base::Base58Btc.decode(data.id.clone())?,
        )?);
        // We expect to find at least one non loop back address
        let p2p_addrs = data
            .addresses
            .iter()
            .map(|addr| -> Multiaddr { addr.parse().expect("should be a valid multiaddr") })
            .filter(|addr| {
                // Address must have both a non loopback ip4 address and a tcp or quic endpoint
                addr.iter().any(|proto| match proto {
                    multiaddr::Protocol::Ip4(ip4) => !ip4.is_loopback(),
                    _ => false,
                })
            })
            // Add peer id to multiaddrs
            .map(|mut addr| {
                addr.push(p2p_proto.clone());
                addr.to_string()
            })
            .collect::<Vec<String>>();

        if !p2p_addrs.is_empty() {
            Ok(IpfsPeerInfo {
                index,
                peer_id: data.id,
                ipfs_rpc_addr: ipfs_rpc_addr.to_owned(),
                p2p_addrs,
            })
        } else {
            Err(anyhow!(
                "peer {} does not have any valid non loopback addresses",
                index
            ))
        }
    }
    async fn peer_status(&self, ipfs_rpc_addr: &str) -> Result<PeerStatus> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/api/v0/swarm/peers", ipfs_rpc_addr))
            .send()
            .await?;
        if !resp.status().is_success() {
            let data: ErrorResponse = resp.json().await?;
            bail!("peer id failed: {}", data.message)
        }

        #[derive(serde::Deserialize)]
        struct Peer {
            //We currently do not care about any of the information of connected peers,
            // just the number of them. As a result this struct does not have any fields.
        }
        #[derive(serde::Deserialize)]
        struct Response {
            #[serde(rename = "Peers")]
            peers: Vec<Peer>,
        }
        let data: Response = resp.json().await?;
        Ok(PeerStatus {
            connected_peers: data.peers.len() as i32,
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimitsSpec {
    /// Cpu resource limit
    pub cpu: Option<Quantity>,
    /// Memory resource limit
    pub memory: Option<Quantity>,
    // Ephemeral storage resource limit
    pub storage: Option<Quantity>,
}

#[derive(Clone)]
pub struct ResourceLimitsConfig {
    /// Cpu resource limit
    pub cpu: Quantity,
    /// Memory resource limit
    pub memory: Quantity,
    // Ephemeral storage resource limit
    pub storage: Quantity,
}

impl ResourceLimitsConfig {
    pub fn from_spec(spec: Option<ResourceLimitsSpec>, defaults: Self) -> Self {
        if let Some(spec) = spec {
            Self {
                cpu: spec.cpu.unwrap_or(defaults.cpu),
                memory: spec.memory.unwrap_or(defaults.memory),
                storage: spec.storage.unwrap_or(defaults.storage),
            }
        } else {
            defaults
        }
    }
}

impl From<ResourceLimitsConfig> for BTreeMap<String, Quantity> {
    fn from(value: ResourceLimitsConfig) -> Self {
        BTreeMap::from_iter([
            ("cpu".to_owned(), value.cpu),
            ("ephemeral-storage".to_owned(), value.storage),
            ("memory".to_owned(), value.memory),
        ])
    }
}
