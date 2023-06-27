//! Defines a common struct for describing a peer.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Peers are identified via a total ordering.
pub type PeerIdx = i32;

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
    /// Report the index of the peer within the total order.
    pub fn index(&self) -> PeerIdx {
        match self {
            Peer::Ceramic(p) => p.index,
            Peer::Ipfs(p) => p.index,
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
    /// The index of the peer within the total order.
    pub index: PeerIdx,
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
/// Describes a peer that only participates using IPFS protocols.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IpfsPeerInfo {
    /// The index of the peer within the total order.
    pub index: PeerIdx,
    /// The public ID of the peer.
    pub peer_id: PeerId,
    /// RPC address of the peer.
    pub ipfs_rpc_addr: String,
    /// Set of p2p addresses of the peer.
    /// Each address contains the /p2p/<peer_id> protocol.
    pub p2p_addrs: Vec<String>,
}
