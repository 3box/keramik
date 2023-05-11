use anyhow::{anyhow, bail, Result};
use keramik_common::peer_info::{PeerId, PeerIdx};
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;

use crate::network::controller::{
    CERAMIC_SERVICE_NAME, CERAMIC_SERVICE_RPC_PORT, CERAMIC_STATEFUL_SET_NAME,
};

#[derive(serde::Deserialize)]
struct ErrorResponse {
    #[serde(rename = "Message")]
    message: String,
}

pub fn peer_rpc_addr(ns: &str, peer: PeerIdx) -> String {
    format!("http://{CERAMIC_STATEFUL_SET_NAME}-{peer}.{CERAMIC_SERVICE_NAME}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_RPC_PORT}")
}

pub async fn peer_addr(ns: &str, peer: PeerIdx) -> Result<(PeerId, String, Vec<String>)> {
    let rpc_addr = peer_rpc_addr(ns, peer);
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v0/id", rpc_addr))
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
            addr.to_string()
        })
        .collect::<Vec<String>>();

    if !addrs.is_empty() {
        Ok((data.id, rpc_addr, addrs))
    } else {
        Err(anyhow!(
            "peer {} does not have any valid non loopback addresses",
            peer
        ))
    }
}
