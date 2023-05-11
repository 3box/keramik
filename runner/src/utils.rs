use anyhow::{bail, Result};
use keramik_common::peer_info::PeerInfo;
use tracing::debug;

/// Initiate connection from peer to other.
#[tracing::instrument(skip_all, fields(peer.index, other.index))]
pub async fn connect_peers(peer: &PeerInfo, other: &PeerInfo) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct ErrorResponse {
        #[serde(rename = "Message")]
        message: String,
    }

    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v0/swarm/connect?{}",
        peer.rpc_addr,
        other
            .p2p_addrs
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
