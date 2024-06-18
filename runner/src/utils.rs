use std::path::Path;

use anyhow::{bail, Result};
use keramik_common::peer_info::Peer;
use rand::seq::SliceRandom;
use rand::thread_rng;
use tokio::{fs::File, io::AsyncReadExt};
use tracing::{debug, error};

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
pub async fn parse_peers_info(path: impl AsRef<Path>) -> Result<Vec<Peer>> {
    let mut f = File::open(path).await?;
    let mut peers_json = String::new();
    f.read_to_string(&mut peers_json).await?;
    Ok(serde_json::from_str(&peers_json)?)
}

/// Calculates the sample size needed to estimate a population proportion with a given confidence level and margin of error.
///
/// # Parameters
/// - `population_size`: The size of the entire population (N).
/// - `confidence_level`: The desired confidence level (e.g., 0.99 for 99% confidence).
/// - `margin_of_error`: The acceptable margin of error (e.g., 0.02 for 2% margin).
///
/// # Returns
/// The calculated sample size (n).
pub async fn calculate_sample_size(
    population_size: usize,
    confidence_level: f64,
    margin_of_error: f64,
) -> usize {
    let mut z = 2.576; // Z-score for 99% confidence interval
    if confidence_level == 0.95 {
        z = 1.96;
    } else if confidence_level == 0.99 {
        z = 2.576;
    } else {
        error!(
            "Invalid confidence level: {}, defaulting to 0.99",
            confidence_level
        );
    }
    let p = 0.5; // Assuming maximum variability
    let e = margin_of_error;

    let sample_size = ((z * z * p * (1.0 - p)) / (e * e)).ceil();
    // We need FPC for small population size (N < 60000) to get a good estimate for larger population size FPC can become irrelevant
    let finite_population_correction =
        sample_size / (1.0 + ((sample_size - 1.0) / (population_size as f64)));
    // We want to round up to the nearest whole number
    finite_population_correction.ceil() as usize
}

/// Selects a random sample of IDs from a given set of IDs.
///
/// # Parameters
/// - `ids`: A slice of IDs to sample from.
/// - `sample_size`: The number of IDs to sample.
///
/// # Returns
/// A vector containing the sampled IDs.
pub async fn select_sample_set_ids<T>(ids: &[T], sample_size: usize) -> Vec<T>
where
    T: Clone,
{
    let mut rng = thread_rng();
    ids.choose_multiple(&mut rng, sample_size)
        .cloned()
        .collect()
}
