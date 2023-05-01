use std::{collections::HashMap, fmt::Display};

use anyhow::{anyhow, bail, Result};
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;
use tracing::error;

/// Peers are identified via a total ordering starting at 0
pub type PeerId = usize;

/// DNS address of peer
pub struct PeerDnsAddr(String);

impl From<PeerId> for PeerDnsAddr {
    fn from(value: PeerId) -> Self {
        Self(format!(
            "ceramic-{}.ceramic.keramik-0.svc.cluster.local",
            value
        ))
    }
}

impl Display for PeerDnsAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
/// HTTP address of peer RPC endpoint
pub struct PeerRpcAddr(String);

impl From<PeerId> for PeerRpcAddr {
    fn from(value: PeerId) -> Self {
        Self(format!("http://{}:5001", PeerDnsAddr::from(value)))
    }
}
impl PeerRpcAddr {
    pub fn as_string(self) -> String {
        self.0
    }
}
impl Display for PeerRpcAddr {
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
pub async fn all_peer_addrs(total: usize) -> Result<HashMap<PeerId, Vec<Multiaddr>>> {
    let mut addrs = HashMap::with_capacity(total);
    for peer in 0..total {
        match peer_addr(peer).await {
            Ok(addr) => {
                addrs.insert(peer, addr);
            }
            Err(err) => error!(%peer, ?err, "failed to lookup peer address"),
        };
    }
    Ok(addrs)
}
#[tracing::instrument]
async fn peer_addr(peer: PeerId) -> Result<Vec<Multiaddr>> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v0/id", PeerRpcAddr::from(peer)))
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
        &multibase::Base::Base58Btc.decode(data.id)?,
    )?);
    // We expect to find at least one non loop back address
    let addrs = data
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
            addr
        })
        .collect::<Vec<Multiaddr>>();

    if !addrs.is_empty() {
        Ok(addrs)
    } else {
        Err(anyhow!(
            "peer {} does not have any valid non loopback addresses",
            peer
        ))
    }
}

/// Initiate connection from peer to other.
#[tracing::instrument]
pub async fn connect_peers(peer: PeerId, other: &[Multiaddr]) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v0/swarm/connect?{}",
        PeerRpcAddr::from(peer),
        other
            .iter()
            .map(|addr| "arg=".to_string() + &addr.to_string())
            .collect::<Vec<String>>()
            .join("&")
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
