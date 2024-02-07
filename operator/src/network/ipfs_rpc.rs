use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use keramik_common::peer_info::IpfsPeerInfo;
use multiaddr::{multihash::Multihash, Multiaddr, Protocol};
use serde::Deserialize;

/// Define the behavior we consume from the IPFS RPC API.
#[async_trait]
pub trait IpfsRpcClient {
    async fn peer_info(&self, ipfs_rpc_addr: &str) -> Result<IpfsPeerInfo>;
    async fn connected_peers(&self, ipfs_rpc_addr: &str) -> Result<Vec<Peer>>;
}

/// Information about connected peers
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Peer {
    pub addr: String,
    pub id: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    #[serde(rename = "Message")]
    message: String,
}

pub struct HttpRpcClient;

#[async_trait]
impl IpfsRpcClient for HttpRpcClient {
    async fn peer_info(&self, ipfs_rpc_addr: &str) -> Result<IpfsPeerInfo> {
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
            .filter_map(|addr| addr.parse::<Multiaddr>().ok())
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
                peer_id: data.id,
                ipfs_rpc_addr: ipfs_rpc_addr.to_owned(),
                p2p_addrs,
            })
        } else {
            Err(anyhow!(
                "peer {ipfs_rpc_addr} does not have any valid non loopback addresses",
            ))
        }
    }
    async fn connected_peers(&self, ipfs_rpc_addr: &str) -> Result<Vec<Peer>> {
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
        struct Response {
            #[serde(rename = "Peers")]
            peers: Option<Vec<Peer>>,
        }
        let data: Response = resp.json().await?;
        Ok(data.peers.unwrap_or_default())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use mockall::mock;

    mock! {
        pub IpfsRpcClientTest {}
        #[async_trait]
        impl IpfsRpcClient for IpfsRpcClientTest {
            async fn peer_info(&self, ipfs_rpc_addr: &str) -> Result<IpfsPeerInfo>;
            async fn connected_peers(&self, ipfs_rpc_addr: &str) -> Result<Vec<Peer>>;
        }
    }
}
