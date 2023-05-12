use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use keramik_common::peer_info::{PeerIdx, PeerInfo};
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;
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
#[unimock(api=RpcClientMock)]
#[async_trait]
pub trait RpcClient {
    async fn peer_info(&self, ns: &str, peer: PeerIdx) -> Result<PeerInfo>;
}

pub struct HttpRpcClient;

fn peer_rpc_addr(ns: &str, peer: PeerIdx) -> String {
    format!("http://{CERAMIC_STATEFUL_SET_NAME}-{peer}.{CERAMIC_SERVICE_NAME}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_IPFS_PORT}")
}
fn ceramic_addr(ns: &str, peer: PeerIdx) -> String {
    format!("http://{CERAMIC_STATEFUL_SET_NAME}-{peer}.{CERAMIC_SERVICE_NAME}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_API_PORT}")
}

#[async_trait]
impl RpcClient for HttpRpcClient {
    async fn peer_info(&self, ns: &str, index: PeerIdx) -> Result<PeerInfo> {
        let ipfs_rpc_addr = peer_rpc_addr(ns, index);
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

        let ceramic_addr = ceramic_addr(ns, index);
        if !p2p_addrs.is_empty() {
            Ok(PeerInfo {
                index,
                peer_id: data.id,
                ipfs_rpc_addr,
                ceramic_addr,
                p2p_addrs,
            })
        } else {
            Err(anyhow!(
                "peer {} does not have any valid non loopback addresses",
                index
            ))
        }
    }
}
