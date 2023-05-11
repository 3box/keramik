//! Defines a common struct for describing a peer.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Peers are identified via a total ordering starting at 0.
pub type PeerIdx = i32;

/// P2p peer id generated via libp2p.
pub type PeerId = String;

/// Describes a peer.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    /// The index of the peer within the total order.
    pub index: PeerIdx,
    /// The public ID of the peer.
    pub peer_id: PeerId,
    /// RPC address of the peer.
    pub rpc_addr: String,
    /// Set of p2p addresses of the peer.
    /// Each address contains the /p2p/<peer_id> protocol.
    pub p2p_addrs: Vec<String>,
}
