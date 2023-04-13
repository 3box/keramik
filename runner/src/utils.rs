use std::{collections::HashMap, fmt::Display};

use anyhow::{anyhow, bail, Result};
use multiaddr::Multiaddr;
use tracing::error;

/// Nodes are identified via a total ordering starting at 0
pub type NodeId = usize;

/// DNS address of Node
pub struct NodeDnsAddr(String);

impl From<NodeId> for NodeDnsAddr {
    fn from(value: NodeId) -> Self {
        Self(format!(
            "ceramic-{}.ceramic.ceramic.svc.cluster.local",
            value
        ))
    }
}

impl Display for NodeDnsAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
/// HTTP address of Node RPC endpoint
pub struct NodeRpcAddr(String);

impl From<NodeId> for NodeRpcAddr {
    fn from(value: NodeId) -> Self {
        Self(format!("http://{}:5001", NodeDnsAddr::from(value)))
    }
}
impl Display for NodeRpcAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// P2p address of Node RPC endpoint
#[derive(Debug)]
pub struct NodeP2pAddr(String);

impl Display for NodeP2pAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(serde::Deserialize)]
struct ErrorResponse {
    #[serde(rename = "Message")]
    message: String,
}

#[tracing::instrument]
pub async fn all_peer_addrs(total: usize) -> Result<HashMap<NodeId, NodeP2pAddr>> {
    let mut addrs = HashMap::with_capacity(total);
    for node in 0..total {
        match peer_addr(node).await {
            Ok(addr) => {
                addrs.insert(node, addr);
            }
            Err(err) => error!(%node, ?err, "failed to lookup peer address"),
        };
    }
    Ok(addrs)
}
#[tracing::instrument]
async fn peer_addr(node: NodeId) -> Result<NodeP2pAddr> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v0/id", NodeRpcAddr::from(node)))
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
    // We expect to find at least one ip4 tcp address
    let addr = data.addresses.iter().find(|addr| {
        let addr: Multiaddr = addr.parse().expect("should be a valid multiaddr");
        // Address must have both a non loopback ip4 address and a TCP endpoint
        addr.iter().any(|proto| match proto {
            multiaddr::Protocol::Ip4(ip4) => !ip4.is_loopback(),
            _ => false,
        }) && addr.iter().any(|proto| match proto {
            multiaddr::Protocol::Tcp(_) => true,
            _ => false,
        })
    });
    if let Some(addr) = addr {
        let addr = NodeP2pAddr(format!("{}/p2p/{}", addr.to_string(), data.id));
        Ok(addr)
    } else {
        Err(anyhow!(
            "peer {} does not have any valid ip4 addresses",
            node
        ))
    }
}

/// Initiate connection from node to other.
#[tracing::instrument]
pub async fn connect_peers(node: NodeId, other: &NodeP2pAddr) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v0/swarm/connect?arg={}",
        NodeRpcAddr::from(node),
        other,
    );
    let resp = client.post(url).send().await?;
    if !resp.status().is_success() {
        let data: ErrorResponse = resp.json().await?;
        bail!("swarm connect failed: {}", data.message)
    }

    #[derive(serde::Deserialize)]
    struct Response {
        #[serde(rename = "Strings")]
        strings: Vec<String>,
    }
    let data: Response = resp.json().await?;
    if let Some(msg) = data.strings.iter().find(|msg| !msg.ends_with("success")) {
        bail!("swarm connect failed: {}", msg)
    }

    Ok(())
}
