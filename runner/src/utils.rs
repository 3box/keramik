use std::{collections::BTreeMap, path::Path};

use anyhow::{bail, Result};
use keramik_common::peer_info::{Peer, PeerIdx};
use tokio::{fs::File, io::AsyncReadExt};
use tracing::debug;

/// Initiate connection from peer to other.
#[tracing::instrument(skip_all, fields(peer.index, other.index))]
pub async fn connect_peers(peer: &Peer, other: &Peer) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct ErrorResponse {
        #[serde(rename = "Message")]
        message: String,
    }

    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v0/swarm/connect?{}",
        peer.ipfs_rpc_addr(),
        other
            .p2p_addrs()
            .iter()
            .map(|addr| "arg=".to_string() + addr)
            .collect::<Vec<String>>()
            .join("&")
    );
    debug!(url, "connect_peers");
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

/// Parse the peers info file.
pub async fn parse_peers_info(path: impl AsRef<Path>) -> Result<BTreeMap<PeerIdx, Peer>> {
    let mut f = File::open(path).await?;
    let mut peers_json = String::new();
    f.read_to_string(&mut peers_json).await?;
    let peers: Vec<Peer> = serde_json::from_str(&peers_json)?;
    Ok(BTreeMap::from_iter(
        peers.into_iter().map(|info| (info.index(), info)),
    ))
}
