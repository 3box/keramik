//! Defines a common struct for describing a peer.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// P2p peer id generated via libp2p.
pub type PeerId = String;

/// Represents a generic Peer in the network.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Peer {
    /// Describes a peer that participates via Ceramic protocols.
    Ceramic(CeramicPeerInfo),
    /// Describes a peer that only participates using IPFS protocols.
    Ipfs(IpfsPeerInfo),
}

impl Peer {
    /// Report the Peer ID of the peer.
    pub fn id(&self) -> &PeerId {
        match self {
            Peer::Ceramic(p) => &p.peer_id,
            Peer::Ipfs(p) => &p.peer_id,
        }
    }
    /// Report address of the IPFS RPC endpoint
    pub fn ipfs_rpc_addr(&self) -> &str {
        match self {
            Peer::Ceramic(p) => &p.ipfs_rpc_addr,
            Peer::Ipfs(p) => &p.ipfs_rpc_addr,
        }
    }
    /// Report peer to peer address of the peer
    pub fn p2p_addrs(&self) -> &[String] {
        match self {
            Peer::Ceramic(p) => p.p2p_addrs.as_slice(),
            Peer::Ipfs(p) => p.p2p_addrs.as_slice(),
        }
    }
}

/// Describes a peer that participates via Ceramic protocols.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CeramicPeerInfo {
    /// The public ID of the peer.
    pub peer_id: PeerId,
    /// RPC address of the peer.
    pub ipfs_rpc_addr: String,
    /// Ceramic API address of the peer.
    pub ceramic_addr: String,
    /// Set of p2p addresses of the peer.
    /// Each address contains the /p2p/<peer_id> protocol.
    pub p2p_addrs: Vec<String>,
}

impl From<CeramicPeerInfo> for IpfsPeerInfo {
    fn from(value: CeramicPeerInfo) -> Self {
        Self {
            peer_id: value.peer_id,
            ipfs_rpc_addr: value.ipfs_rpc_addr,
            p2p_addrs: value.p2p_addrs,
        }
    }
}
/// Describes a peer that only participates using IPFS protocols.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IpfsPeerInfo {
    /// The public ID of the peer.
    pub peer_id: PeerId,
    /// RPC address of the peer.
    pub ipfs_rpc_addr: String,
    /// Set of p2p addresses of the peer.
    /// Each address contains the /p2p/<peer_id> protocol.
    pub p2p_addrs: Vec<String>,
}
