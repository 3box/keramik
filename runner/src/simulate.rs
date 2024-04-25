use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use clap::{Args, ValueEnum};
use goose::{config::GooseConfiguration, prelude::GooseMetrics, GooseAttack};
use keramik_common::peer_info::Peer;
use opentelemetry::{
    global,
    metrics::{ObservableGauge, Observer},
    KeyValue,
};
use reqwest::Url;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::{
    scenario::{
        ceramic::{self, new_streams},
        get_redis_client, ipfs_block_fetch, recon_sync,
    },
    utils::parse_peers_info,
    CommandResult,
};

// FIXME: is it worth attaching metrics to the peer info?
const IPFS_SERVICE_METRICS_PORT: &str = "9465";
const EVENT_SYNC_METRIC_NAME: &str = "ceramic_store_key_insert_count_total";
const ANCHOR_REQUEST_MIDS_KEY: &str = "anchor_mids";
const CAS_ANCHOR_REQUEST_KEY: &str = "anchor_requests";

/// Options to Simulate command
#[derive(Args, Debug)]
pub struct Opts {
    /// Unique name for this simulation run.
    #[arg(long, env = "SIMULATE_NAME")]
    name: String,

    /// Simulation scenario to run.
    #[arg(long, value_enum, env = "SIMULATE_SCENARIO")]
    scenario: Scenario,

    /// Id of the peer to target.
    #[arg(long, env = "SIMULATE_MANAGER")]
    manager: bool,

    /// Index into peers list of the peer to target.
    #[arg(long, env = "SIMULATE_TARGET_PEER")]
    target_peer: usize,

    /// Path to file containing the list of peers.
    /// File should contian JSON encoding of Vec<Peer>.
    #[arg(long, env = "SIMULATE_PEERS_PATH")]
    peers: PathBuf,

    /// Number of users to simulate on each node. The total number of users
    /// running the test scenario will be this value * N nodes.
    ///
    /// Implmentation details: A user corresponds to a tokio task responsible
    /// for making requests. They should have low memory overhead, so you can
    /// create many users and then use `throttle_requests` to constrain the overall
    /// throughput on the node (specifically the HTTP requests made).
    #[arg(long, default_value_t = 4, env = "SIMULATE_USERS")]
    users: usize,

    /// Duration of the simulation
    #[arg(long, env = "SIMULATE_RUN_TIME", default_value = "10m")]
    run_time: String,

    /// Whether to log the scenario tx, req, errors to a file at a given level.
    #[arg(long, env = "SIMULATE_LOG_LEVEL")]
    log_level: Option<LogLevel>,

    /// Unique value per test run to ensure uniqueness across different test runs.
    /// All workers and manager must be given the same nonce.
    #[arg(long, env = "SIMULATE_NONCE")]
    nonce: u64,

    /// Option to throttle requests (per second) for load control
    #[arg(long, env = "SIMULATE_THROTTLE_REQUESTS")]
    throttle_requests: Option<usize>,

    /// Request target for the scenario to be a success. Scenarios can use this to
    /// validate throughput and correctness before returning. The exact definition is
    /// left to the scenario (requests per second, total requests, rps/node etc).
    #[arg(long, env = "SIMULATE_TARGET_REQUESTS")]
    target_request_rate: Option<usize>,
    // #[arg(long, env = "SIMULATE_ANCHOR_WAIT_TIME")]
    // anchor_wait_time: Option<u64>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum LogLevel {
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn as_goose_log_level(&self) -> u8 {
        match self {
            LogLevel::Warn => 0,
            LogLevel::Info => 1,
            LogLevel::Debug => 2,
            LogLevel::Trace => 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Topology {
    pub target_worker: usize,
    pub total_workers: usize,
    pub users: usize,
    pub nonce: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum AnchorStatus {
    Anchored,
    Pending,
    Failed,
    NotRequested,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct AnchorMetricsState {
    #[serde(rename = "anchorStatus")]
    anchor_status: AnchorStatus,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct AnchorMetrics {
    state: AnchorMetricsState,
}

#[derive(Clone, Debug, Copy, ValueEnum)]
pub enum Scenario {
    /// Queries the Id of the IPFS peers.
    IpfsRpc,
    /// Simple Ceramic Scenario that creates two models and MIDs and then and gets the streams
    /// and then replaces the existing model instance document with a new one. It uses one DID for all users,
    /// and creates a new model instance document for each user.
    CeramicSimple,
    /// Same requests as the CeramicSimple scenario but changes the DID and MID ownership. Every user has their own
    /// DID and creates a Model Instance Document, that they then replace throughout the scenario.
    CeramicModelReuse,
    /// WriteOnly Ceramic Scenario that creates two models and replaces the model instance documents.
    CeramicWriteOnly,
    /// New Streams Ceramic Scenario. Only creates new model instance documents rather than updating anything.
    CeramicNewStreams,
    /// Scenario that creates new model instance documents that are about 1kb in size and verifies that they sync
    /// to the other nodes. This is a benchmark scenario for e2e testing, simliar to the recon event sync scenario,
    /// but covering using js-ceramic rather than talking directly to the ipfs API.
    CeramicNewStreamsBenchmark,
    /// Simple Query Scenario. Creates multiple MIDs for a model and then queries the model instance documents,
    /// updates the document, and then queries again to verify the update was persisted.
    CeramicQuery,
    /// Nodes subscribe to same model. One node generates new events, recon syncs event keys and data to peers.
    ReconEventSync,
    /// Nodes subscribe to same model. One node generates new events, recon syncs event keys to peers.
    /// Which of the Recon scenarios  you should choose is dictated by the API of the ceramic-one instance
    /// being used. Previously, it only supported keys but newer versions support keys and data.
    /// This scenario is for the keys only version and will fail on new verisons.
    ReconEventKeySync,
    // Scenario that creates model instance documents and verifies that they have been anchored at the desired rate.
    // This is a benchmark scenario for e2e testing, simliar to the recon event sync scenario,
    // but covering using js-ceramic rather than talking directly to the ipfs API.
    CeramicAnchoringBenchmark,

    // Scenario that creates model instance documents and verifies that they have been anchored at the desired rate.
    // This is a benchmark scenario for e2e testing, simliar to the recon event sync scenario,
    // but covering using js-ceramic rather than talking directly to the ipfs API.
    CASBenchmark,
}

impl Scenario {
    pub fn name(&self) -> &'static str {
        match self {
            Scenario::IpfsRpc => "ipfs_rpc",
            Scenario::CeramicSimple => "ceramic_simple",
            Scenario::CeramicWriteOnly => "ceramic_write_only",
            Scenario::CeramicNewStreams => "ceramic_new_streams",
            Scenario::CeramicNewStreamsBenchmark => "ceramic_new_streams_benchmark",
            Scenario::CeramicQuery => "ceramic_query",
            Scenario::CeramicModelReuse => "ceramic_model_reuse",
            Scenario::ReconEventSync => "recon_event_sync",
            Scenario::ReconEventKeySync => "recon_event_key_sync",
            Scenario::CeramicAnchoringBenchmark => "ceramic_anchoring_benchmark",
            Scenario::CASBenchmark => "cas_benchmark",
        }
    }

    fn target_addr(&self, peer: &Peer) -> Result<String> {
        match self {
            Self::IpfsRpc | Self::ReconEventSync | Self::ReconEventKeySync => {
                Ok(peer.ipfs_rpc_addr().to_owned())
            }
            Self::CeramicSimple
            | Self::CeramicWriteOnly
            | Self::CeramicNewStreams
            | Self::CeramicAnchoringBenchmark
            | Self::CeramicNewStreamsBenchmark
            | Self::CeramicQuery
            | Self::CASBenchmark
            | Self::CeramicModelReuse => Ok(peer
                .ceramic_addr()
                .ok_or_else(|| {
                    anyhow!(
                        "cannot use non ceramic peer as target for simulation {}",
                        self.name(),
                    )
                })?
                .to_owned()),
        }
    }
}

type MetricsCollector = Box<dyn MetricsCollection + Send + Sync>;

#[async_trait::async_trait]
trait MetricsCollection: std::fmt::Debug {
    /// Collects a counter metric from the given host. None if metric not found.
    async fn collect_counter(&self, addr: Url, metric_name: &str) -> Result<Option<u64>>;
    fn boxed(&self) -> MetricsCollector;
}

#[derive(Clone, Debug)]
struct PromMetricCollector {
    client: reqwest::Client,
}

#[async_trait::async_trait]
impl MetricsCollection for PromMetricCollector {
    async fn collect_counter(&self, addr: Url, metric_name: &str) -> Result<Option<u64>> {
        let resp = self.client.get(addr.clone()).send().await?;
        if !resp.status().is_success() {
            warn!(?resp, "metrics request failed for peer {}", addr);
            bail!("Failed to get metrics for host: {}", addr);
        } else {
            let body = resp.text().await?;
            for metric in body.lines() {
                match metric.split(' ').collect::<Vec<&str>>().as_slice() {
                    [name, value] if *name == metric_name => {
                        let val = value.parse::<u64>().map_err(|e| {
                            warn!(metric=%name, %value, "failed to parse metric: {}", e);
                            e
                        })?;
                        return Ok(Some(val));
                    }
                    _ => {}
                }
            }

            warn!("Failed to find metric {} for host: {}", metric_name, addr);
            Ok(None)
        }
    }
    fn boxed(&self) -> MetricsCollector {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
struct PeerRequestMetricInfo {
    pub name: String,
    pub count: u64,
    pub runtime: Option<u64>,
}

impl PeerRequestMetricInfo {
    pub fn new(name: String, count: u64, runtime: Option<u64>) -> Self {
        Self {
            name,
            count,
            runtime,
        }
    }

    pub fn rps(&self) -> f64 {
        match self.runtime {
            Some(0) => {
                warn!("Runtime of 0 seconds is invalid for RPS calculation");
                0.0
            }
            Some(runtime) => self.count as f64 / runtime as f64,
            None => {
                warn!("Runtime is missing for RPS calculation");
                0.0
            }
        }
    }
}

pub(crate) fn parse_base_url_to_pod(url: &reqwest::Url) -> Result<String> {
    // addr looks like http://ceramic-0-0.ceramic-0.keramik-small.svc.cluster.local:7007
    // parse it to get the pod name (ceramic-0-0)
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("failed to parse host from url: {}", url))?;
    let parts: Vec<&str> = host.split('.').collect();
    let name = parts
        .first()
        .ok_or_else(|| anyhow!("failed to parse pod name from host: {}", host))?;
    Ok(name.to_string())
}

fn final_peer_req_metric_info(
    before: &[PeerRequestMetricInfo],
    after: &[PeerRequestMetricInfo],
    run_time_seconds: u64,
) -> Result<Vec<PeerRequestMetricInfo>> {
    let mut before = before.to_vec();
    before.sort_by(|a, b| a.name.cmp(&b.name));
    let mut after = after.to_vec();
    after.sort_by(|a, b| a.name.cmp(&b.name));

    if before.len() != after.len() {
        warn!(?before, ?after, "Mismatched peer metrics",);
        bail!(
            "Mismatched peer metrics: before: {}, after: {}",
            before.len(),
            after.len()
        );
    }

    before
        .into_iter()
        .zip(after.iter())
        .map(|(before, current)| {
            if before.name == current.name {
                Ok(PeerRequestMetricInfo::new(
                    before.name.to_owned(),
                    current.count - before.count,
                    Some(run_time_seconds),
                ))
            } else {
                Err(anyhow!(
                    "Mismatched peer metrics: before: {}, after: {}",
                    before.name,
                    current.name
                ))
            }
        })
        .collect::<anyhow::Result<Vec<PeerRequestMetricInfo>>>()
}

#[derive(Debug, Clone)]
struct PeerRps(Vec<PeerRequestMetricInfo>);

impl PeerRps {
    pub fn new(rps: Vec<PeerRequestMetricInfo>) -> Self {
        Self(rps)
    }

    fn min(&self) -> f64 {
        let x = self.0.iter().min_by(|a, b| {
            let a_rps = a.rps();
            let b_rps = b.rps();
            if a_rps.is_normal() && b_rps.is_normal() {
                a_rps.partial_cmp(&b_rps).unwrap()
            } else {
                std::cmp::Ordering::Greater
            }
        });
        x.map_or(0.0, |p| p.rps())
    }
}
/// This struct holds information about the state of the simulation that
/// allows us to determine whether or not we met our success criteria.
#[derive(Debug)]
struct ScenarioState {
    pub topo: Topology,
    pub peers: Vec<Peer>,
    pub manager: bool,
    pub scenario: Scenario,
    pub target_request_rate: Option<usize>,
    metrics_collector: MetricsCollector,
    before_metrics: Option<Vec<PeerRequestMetricInfo>>,
    run_time: String,
    log_level: Option<LogLevel>,
    throttle_requests: Option<usize>,
    // wait_time: Option<u64>,
}

impl ScenarioState {
    /// Peers override is for testing only
    async fn try_from_opts(
        opts: Opts,
        metrics_collector: MetricsCollector,
        peers_override: Option<Vec<Peer>>, // for testing
    ) -> Result<Self> {
        // We assume exactly one worker per peer.
        // This allows us to be deterministic in how each user operates.
        tracing::debug!(?opts, "building state from opts");
        let peers: Vec<Peer> = if let Some(peers) = peers_override {
            peers
        } else {
            parse_peers_info(opts.peers.clone())
                .await?
                .into_iter()
                .filter(|peer| matches!(peer, Peer::Ceramic(_)))
                .collect()
        };
        if peers.is_empty() {
            bail!("No peers found in peers file: {}", opts.peers.display());
        }
        let topo = Topology {
            target_worker: opts.target_peer,
            total_workers: peers.len(),
            nonce: opts.nonce,
            users: peers.len() * opts.users,
        };
        Ok(Self {
            topo,
            peers,
            metrics_collector,
            manager: opts.manager,
            scenario: opts.scenario,
            target_request_rate: opts.target_request_rate,
            before_metrics: None,
            run_time: opts.run_time,
            log_level: opts.log_level,
            throttle_requests: opts.throttle_requests,
            // wait_time: opts.anchor_wait_time,
        })
    }

    async fn build_goose_scenario(&mut self) -> Result<goose::prelude::Scenario> {
        let scenario = match self.scenario {
            Scenario::IpfsRpc => ipfs_block_fetch::scenario(self.topo)?,
            Scenario::CeramicSimple => ceramic::simple::scenario(self.scenario.into()).await?,
            Scenario::CeramicModelReuse => ceramic::simple::scenario(self.scenario.into()).await?,
            Scenario::CeramicWriteOnly => {
                ceramic::write_only::scenario(self.scenario.into()).await?
            }
            Scenario::CeramicNewStreams | Scenario::CeramicAnchoringBenchmark => {
                ceramic::new_streams::small_large_scenario(self.scenario.into()).await?
            }
            Scenario::CeramicNewStreamsBenchmark => {
                ceramic::new_streams::benchmark_scenario(self.scenario.into(), self.topo.nonce)
                    .await?
            }
            Scenario::CeramicQuery => ceramic::query::scenario(self.scenario.into()).await?,
            Scenario::ReconEventSync => recon_sync::event_sync_scenario().await?,
            Scenario::ReconEventKeySync => recon_sync::event_key_sync_scenario().await?,
            Scenario::CASBenchmark => ceramic::anchor::cas_benchmark().await?,
        };
        self.collect_before_metrics().await?;
        Ok(scenario)
    }

    fn target_peer_addr(&self) -> Result<String> {
        self.scenario.target_addr(
            self.peers
                .get(self.topo.target_worker)
                .ok_or_else(|| anyhow!("target peer too large, not enough peers"))?,
        )
    }

    /// Returns the counter value (or None) for each peer in order of the peers list
    async fn get_peers_counter_metric(
        &self,
        metric_name: &str,
        metrics_path: &str, // may include the port and path
    ) -> Result<Vec<PeerRequestMetricInfo>> {
        // This is naive and specific to our requirement of getting a prometheus counter.
        let mut results = Vec::with_capacity(self.peers.len());
        for peer in self.peers.iter() {
            // Ideally, the peer stores the available metrics endpoints and we don't need to build them,
            // but that's not the case right now and the ceramic/recon split makes it a bit odd, as most
            // scenarios care about the ceramic metrics, but the recon metrics are on the IPFS port.
            if let Some(addr) = peer.ceramic_addr() {
                let addr = addr.parse::<reqwest::Url>()?;
                if let Some(host) = addr.host_str() {
                    let url = format!("http://{}:{}", host, metrics_path);
                    let metric = self
                        .metrics_collector
                        .collect_counter(url.parse()?, metric_name)
                        .await?;
                    if let Some(m) = metric {
                        results.push(PeerRequestMetricInfo::new(
                            parse_base_url_to_pod(&addr)?,
                            m,
                            None,
                        ));
                    } else {
                        results.push(PeerRequestMetricInfo::new(
                            parse_base_url_to_pod(&addr)?,
                            0, // Use 0 as the count if the metric is not found
                            None,
                        ));
                    }
                } else {
                    warn!("Failed to parse ceramic addr for host: {}", addr);
                    bail!("Failed to collect metric from for host: {}", addr);
                }
            }
        }
        Ok(results)
    }

    async fn collect_before_metrics(&mut self) -> Result<()> {
        if !self.manager {
            Ok(())
        } else {
            match self.scenario {
                Scenario::IpfsRpc
                | Scenario::CeramicSimple
                | Scenario::CeramicWriteOnly
                | Scenario::CeramicNewStreams
                | Scenario::CeramicQuery
                | Scenario::CASBenchmark
                | Scenario::CeramicModelReuse => Ok(()),
                Scenario::CeramicAnchoringBenchmark | Scenario::CeramicNewStreamsBenchmark => {
                    // we collect things in the scenario and use redis to decide success/failure of the test
                    Ok(())
                }
                Scenario::ReconEventSync | Scenario::ReconEventKeySync => {
                    let peers = self
                        .get_peers_counter_metric(EVENT_SYNC_METRIC_NAME, IPFS_SERVICE_METRICS_PORT)
                        .await?;

                    self.before_metrics = Some(peers);
                    Ok(())
                }
            }
        }
    }

    /// For now, most scenarios are successful if they complete without error and only EventIdSync has a criteria.
    /// Not a result to ensure we always proceed with cleanup, even if we fail to validate the scenario.
    /// Should return the Minimum RPS of all peers as the f64
    pub async fn validate_scenario_success(
        &self,
        metrics: &GooseMetrics,
    ) -> (CommandResult, Option<PeerRps>) {
        if !self.manager {
            return (CommandResult::Success, None);
        }
        match self.scenario {
            Scenario::IpfsRpc
            | Scenario::CeramicSimple
            | Scenario::CeramicWriteOnly
            | Scenario::CeramicNewStreams
            | Scenario::CeramicQuery
            | Scenario::CeramicModelReuse => (CommandResult::Success, None),
            Scenario::CeramicNewStreamsBenchmark => {
                let res =
                    new_streams::benchmark_scenario_metrics(self.peers.len(), self.topo.nonce)
                        .await;
                if res.is_empty() {
                    return (
                        CommandResult::Failure(anyhow!("No metrics collected")),
                        None,
                    );
                }
                let target = self.target_request_rate.unwrap_or(300);
                let mut errors = vec![];
                let mut rps_list = vec![];
                for (name, count) in res {
                    let metric = PeerRequestMetricInfo::new(
                        name.clone(),
                        count as u64,
                        Some(metrics.duration as u64),
                    );
                    let rps = metric.rps();
                    rps_list.push(metric);
                    if rps < target as f64 {
                        let msg = format!(
                            "Worker {} did not meet the target request rate: {} < {} (total requests: {} over {})",
                            name, rps, target, count, metrics.duration
                        );
                        warn!(msg);
                        errors.push(msg);
                    } else {
                        info!("worker {} met threshold! {} > {}", name, rps, target);
                    }
                }
                let min = if rps_list.is_empty() {
                    None
                } else {
                    Some(PeerRps::new(rps_list))
                };
                if errors.is_empty() {
                    (CommandResult::Success, min)
                } else {
                    (CommandResult::Failure(anyhow!(errors.join("\n"))), min)
                }
            }
            Scenario::ReconEventSync | Scenario::ReconEventKeySync => {
                // It'd be easy to make work for other scenarios if they defined a rate and metric. However, the scenario we're
                // interested in is asymmetrical in what the workers do, and we're trying to look at what happens to other nodes,
                // which is not how most scenarios work. It also uses the IPFS metrics endpoint. We could parameterize or use a
                // trait, but we don't yet have a use case, and might need to use transactions, or multiple requests, or something
                // entirely different. Anyway, to avoid generalizing the exception we keep it simple.
                let req_name = recon_sync::CREATE_EVENT_REQ_NAME;

                let metric = match metrics
                    .requests
                    .get(req_name)
                    .ok_or_else(|| anyhow!("failed to find goose metrics for request {}", req_name))
                    .map_err(CommandResult::Failure)
                {
                    Ok(v) => v,
                    Err(e) => return (e, None),
                };

                self.validate_recon_scenario_success_int(
                    metrics.duration as u64,
                    metric.success_count as u64,
                )
                .await
            }
            Scenario::CeramicAnchoringBenchmark => {
                match self.validate_anchoring_benchmark_scenario_success().await {
                    Ok(result) => result,
                    Err(e) => (CommandResult::Failure(e), None),
                }
            }
            Scenario::CASBenchmark => {
                let ids = self
                    .get_set_from_redis(CAS_ANCHOR_REQUEST_KEY)
                    .await
                    .unwrap();
                info!("Number of CAS anchoring requests: {}", ids.len());
                self.remove_stream_from_redis(CAS_ANCHOR_REQUEST_KEY, ids)
                    .await
                    .unwrap();
                (CommandResult::Success, None)
            }
        }
    }

    pub async fn get_anchor_status(
        &self,
        peer: &Peer,
        stream_id: String,
    ) -> Result<AnchorStatus, anyhow::Error> {
        let client = reqwest::Client::new();
        let ceramic_addr = peer
            .ceramic_addr()
            .ok_or_else(|| anyhow!("Peer does not have a ceramic address"))?;

        let streams_url = format!("{}/{}/{}", ceramic_addr, "api/v0/streams", stream_id);

        let response = client
            .get(streams_url)
            .send()
            .await
            .map_err(|e| {
                error!("HTTP request failed: {}", e);
                e
            })?
            .error_for_status()
            .map_err(|e| {
                error!("HTTP request returned unsuccessful status: {}", e);
                e
            })?;

        let metrics = response.json::<AnchorMetrics>().await.map_err(|e| {
            error!("Failed to parse anchor metrics response: {}", e);
            e
        })?;
        Ok(metrics.state.anchor_status)
    }

    pub async fn get_set_from_redis(&self, key: &str) -> Result<Vec<String>, anyhow::Error> {
        // Create a new Redis client
        let client: redis::Client = get_redis_client().await?;
        let mut connection = client.get_async_connection().await?;
        // Get the MIDs from Redis
        let response = redis::cmd("SMEMBERS")
            .arg(key)
            .query_async(&mut connection)
            .await?;
        // Return the MIDs
        Ok(response)
    }

    pub async fn remove_stream_from_redis(
        &self,
        key: &str,
        stream_ids: Vec<String>,
    ) -> Result<i32, anyhow::Error> {
        let client: redis::Client = get_redis_client().await?;
        let mut connection = client.get_async_connection().await?;
        let response = redis::cmd("SREM")
            .arg(key)
            .arg(stream_ids)
            .query_async(&mut connection)
            .await?;
        Ok(response)
    }

    async fn validate_anchoring_benchmark_scenario_success(
        &self,
    ) -> Result<(CommandResult, Option<PeerRps>), anyhow::Error> {
        let mut anchored_count = 0;
        let mut pending_count = 0;
        let mut failed_count = 0;
        let mut not_requested_count = 0;
        let wait_duration = Duration::from_secs(60 * 60);
        // TODO_3164_1 : Make this a parameter, pass it in from the scenario config
        // TODO_3164_3 : Code clean-up : Move redis calls to separate file move it out of simulate.rs
        // TODO_3164_4 : Code clean-up : Move api call (fetch stream) to model_instance.rs?, maybe rename it
        sleep(wait_duration).await;

        // Pick a peer at random
        let peer = self.peers.first().unwrap();

        let ids = self.get_set_from_redis(ANCHOR_REQUEST_MIDS_KEY).await?;
        info!("Number of MIDs: {}", ids.len());

        // Make an API call to get the status of request from the chosen peer
        for stream_id in ids.clone() {
            info!("Fetching anchor status for streamID {}", stream_id);
            match self.get_anchor_status(peer, stream_id.clone()).await {
                Ok(AnchorStatus::Anchored) => anchored_count += 1,
                Ok(AnchorStatus::Pending) => pending_count += 1,
                Ok(AnchorStatus::Failed) => failed_count += 1,
                Ok(AnchorStatus::NotRequested) => not_requested_count += 1,
                Err(e) => {
                    failed_count += 1;
                    error!("Failed to get anchor status: {}", e);
                }
            }
        }

        // remove stream ids from redis
        self.remove_stream_from_redis(ANCHOR_REQUEST_MIDS_KEY, ids)
            .await?;

        // Log the counts
        info!("Not requested count: {:2}", not_requested_count);
        info!("Anchored count: {:2}", anchored_count);
        info!("Pending count: {:2}", pending_count);
        info!("Failed count: {:2}", failed_count);

        if failed_count > 0 {
            Ok((
                CommandResult::Failure(anyhow!("Failed count is greater than 0")),
                None,
            ))
        } else {
            info!("Anchored count is : {}", anchored_count);
            Ok((CommandResult::Success, None))
        }
        // TODO_3164_2 : Report these counts to Graphana
    }

    /// Removed from `validate_scenario_success` to make testing easier as constructing the GooseMetrics appropriately is difficult
    /// Should return the Minimum RPS of all peers as the f64
    async fn validate_recon_scenario_success_int(
        &self,
        run_time_seconds: u64,
        request_cnt: u64,
    ) -> (CommandResult, Option<PeerRps>) {
        if !self.manager {
            return (CommandResult::Success, None);
        }
        match self.scenario {
            Scenario::IpfsRpc
            | Scenario::CeramicSimple
            | Scenario::CeramicWriteOnly
            | Scenario::CeramicNewStreams
            | Scenario::CeramicQuery
            | Scenario::CeramicModelReuse
            | Scenario::CeramicAnchoringBenchmark
            | Scenario::CASBenchmark
            | Scenario::CeramicNewStreamsBenchmark => (CommandResult::Success, None),
            Scenario::ReconEventSync | Scenario::ReconEventKeySync => {
                let default_rate = 300;
                let metric_name = EVENT_SYNC_METRIC_NAME;

                let peer_req_cnts = match self
                    .get_peers_counter_metric(metric_name, IPFS_SERVICE_METRICS_PORT)
                    .await
                    .map_err(CommandResult::Failure)
                {
                    Ok(v) => v,
                    Err(e) => return (e, None),
                };

                // There is no `f64::try_from::<u64 or usize>` but if these values don't fit, we have bigger problems
                let threshold = self.target_request_rate.unwrap_or(default_rate) as f64;
                let create_rps = request_cnt as f64 / run_time_seconds as f64;

                let before_metrics = match self
                    .before_metrics
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow!(
                            "failed to get before metrics for scenario {}",
                            self.scenario.name()
                        )
                    })
                    .map_err(CommandResult::Failure)
                {
                    Ok(v) => v,
                    Err(e) => return (e, None),
                };

                // For now, assume writer and all peers must meet the threshold rate
                let peer_metrics = match final_peer_req_metric_info(
                    before_metrics,
                    &peer_req_cnts,
                    run_time_seconds,
                ) {
                    Ok(v) => v,
                    Err(e) => return (CommandResult::Failure(e), None),
                };

                let mut errors = peer_metrics.iter().flat_map(|p| {
                    let rps = p.rps();
                    if rps < threshold {
                        warn!(current_req_cnt=%p.count, run_time_seconds=?p.runtime, %threshold, %rps, "rps less than threshold");
                        Some(
                            format!(
                                "Peer {} RPS less than threshold: {} < {}",
                                p.name, rps, threshold
                            ),
                        )
                    } else {
                        info!(current_req_cnt=%p.count, before=%p.count, run_time_seconds=?p.runtime, %threshold, %rps, "success! peer {} over the threshold", p.name);
                        None
                    }
                }
                ).collect::<Vec<String>>();

                let peer_rps = if peer_metrics.is_empty() {
                    None
                } else {
                    Some(PeerRps::new(peer_metrics))
                };

                if create_rps < threshold {
                    warn!(
                        ?create_rps,
                        ?threshold,
                        "create rps less than threshold on writer node"
                    );
                    errors.push(format!(
                        "Create event RPS less than threshold on writer node: {} < {}",
                        create_rps, threshold
                    ));
                }
                if errors.is_empty() {
                    info!(
                        ?create_rps,
                        ?threshold,
                        "SUCCESS! All peers met the threshold"
                    );
                    (CommandResult::Success, peer_rps)
                } else {
                    warn!(?errors, "FAILURE! Not all peers met the threshold");
                    (CommandResult::Failure(anyhow!(errors.join("\n"))), peer_rps)
                }
            }
        }
    }

    fn goose_config(&self) -> Result<GooseConfiguration> {
        let config = if self.manager {
            let mut config = GooseConfiguration::default();
            if let Some(ref log_level) = self.log_level {
                // wow this is annoying but we're going to match goose internals here
                match log_level {
                    LogLevel::Warn => {
                        config.verbose = 0;
                        config.quiet = 1;
                    }
                    LogLevel::Info => {
                        config.verbose = 0;
                        config.quiet = 0;
                    }
                    LogLevel::Debug => {
                        config.verbose = 1;
                    }
                    LogLevel::Trace => {
                        config.verbose = 2;
                    }
                }
            }
            config.users = Some(self.topo.users);
            config.manager = true;
            config.manager_bind_port = 5115;
            config.expect_workers = Some(self.topo.total_workers);
            config.startup_time = "10s".to_owned();
            config.run_time = self.run_time.clone();
            config
        } else {
            let mut config = GooseConfiguration::default();
            // we could set `config.verbose` which is for stdout, but for now we just use files as it doesn't seem to do anything on workers
            if let Some(ref log_level) = self.log_level {
                config.log_level = log_level.as_goose_log_level();
                config.scenario_log = "scenario.log".to_owned();
                config.transaction_log = "transaction.log".to_owned();
                config.request_log = "request.log".to_owned();
                config.error_log = "error.log".to_owned();
            }
            config.worker = true;
            config.host = self.target_peer_addr()?;
            // We are leveraging k8s dns search path so we do not have to specify the fully qualified
            // domain name explicitly.
            config.manager_host = "manager.goose".to_owned();
            config.manager_port = 5115;
            if let Some(throttle_requests) = self.throttle_requests {
                config.throttle_requests = throttle_requests
            }
            config
        };
        Ok(config)
    }
}

#[tracing::instrument]
pub async fn simulate(opts: Opts) -> Result<CommandResult> {
    let mut metrics = Metrics::init(&opts)?;

    let metrics_collector = Box::new(PromMetricCollector {
        client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()?,
    });
    let mut state = ScenarioState::try_from_opts(opts, metrics_collector, None).await?;
    let scenario = state.build_goose_scenario().await?;
    let config: GooseConfiguration = state.goose_config()?;

    let goose_metrics = match GooseAttack::initialize_with_config(config)?
        .register_scenario(scenario)
        .execute()
        .await
    {
        Ok(m) => m,
        Err(e) => {
            error!("{:#?}", e);
            return Err(e.into());
        }
    };

    let (success, peer_rps) = state.validate_scenario_success(&goose_metrics).await;
    metrics.record(goose_metrics, peer_rps);

    Ok(success)
}

struct Metrics {
    inner: Arc<Mutex<MetricsInner>>,
}
struct MetricsInner {
    goose_metrics: Option<GooseMetrics>,
    peer_rps: Option<PeerRps>,

    attrs: Vec<KeyValue>,

    duration: ObservableGauge<u64>,
    maximum_users: ObservableGauge<u64>,
    users_total: ObservableGauge<u64>,

    scenarios_total: ObservableGauge<u64>,
    scenarios_duration_percentiles: ObservableGauge<f64>,

    txs_total: ObservableGauge<u64>,
    txs_duration_percentiles: ObservableGauge<f64>,

    requests_total: ObservableGauge<u64>,
    requests_status_codes_total: ObservableGauge<u64>,
    requests_duration_percentiles: ObservableGauge<f64>,

    simulation_min_peer_requests_per_second: ObservableGauge<f64>,
    simulation_peer_requests_per_second: ObservableGauge<f64>,
}

impl Metrics {
    fn init(opts: &Opts) -> Result<Self> {
        let mut attrs = vec![
            KeyValue::new("simulation", opts.name.clone()),
            KeyValue::new("scenario", opts.scenario.name()),
            KeyValue::new("nonce", opts.nonce.to_string()),
            KeyValue::new("mode", if opts.manager { "manager" } else { "worker" }),
        ];
        if !opts.manager {
            attrs.push(KeyValue::new("worker_id", opts.target_peer.to_string()));
        }

        let meter = global::meter("simulate");
        let duration = meter
            .u64_observable_gauge("goose_duration")
            .with_description("Total number of seconds the load test ran")
            .init();
        let maximum_users = meter
            .u64_observable_gauge("goose_maximum_users")
            .with_description("Maximum number of users simulated during this load test")
            .init();
        let users_total = meter
            .u64_observable_gauge("goose_total_users")
            .with_description("Total number of users simulated during this load test")
            .init();

        // Scenario specific metrics
        let scenarios_total = meter
            .u64_observable_gauge("goose_scenarios_total")
            .with_description("Total number of scenario runs")
            .init();
        let scenarios_duration_percentiles = meter
            .f64_observable_gauge("goose_scenarios_duration_percentiles")
            .with_description("Specific percentiles of scenario durations")
            .init();

        // Transaction specific metrics
        let txs_total = meter
            .u64_observable_gauge("goose_txs_total")
            .with_description("Total number of transaction runs")
            .init();
        let txs_duration_percentiles = meter
            .f64_observable_gauge("goose_txs_duration_percentiles")
            .with_description("Specific percentiles of transaction durations")
            .init();

        // Request specific metrics
        let requests_total = meter
            .u64_observable_gauge("goose_requests_total")
            .with_description("Total number of requests")
            .init();
        let requests_status_codes_total = meter
            .u64_observable_gauge("goose_requests_status_codes_total")
            .with_description("Total number of requests with a status code")
            .init();
        let requests_duration_percentiles = meter
            .f64_observable_gauge("goose_requests_duration_percentiles")
            .with_description("Specific percentiles of request durations")
            .init();
        let simulation_min_peer_requests_per_second = meter
            .f64_observable_gauge("simulation_min_peer_requests_per_second")
            .with_description(
                "Minimum by peer of the average request per second during a simulation run",
            )
            .init();

        let simulation_peer_requests_per_second = meter
            .f64_observable_gauge("simulation_peer_requests_per_second")
            .with_description("Peer average request per second during a simulation run")
            .init();

        let instruments = [
            duration.as_any(),
            maximum_users.as_any(),
            users_total.as_any(),
            scenarios_total.as_any(),
            scenarios_duration_percentiles.as_any(),
            txs_total.as_any(),
            txs_duration_percentiles.as_any(),
            requests_total.as_any(),
            requests_status_codes_total.as_any(),
            requests_duration_percentiles.as_any(),
            simulation_peer_requests_per_second.as_any(),
            simulation_min_peer_requests_per_second.as_any(),
        ];
        let inner = Arc::new(Mutex::new(MetricsInner {
            goose_metrics: None,
            peer_rps: None,
            attrs,
            duration,
            maximum_users,
            users_total,
            scenarios_total,
            scenarios_duration_percentiles,
            txs_total,
            txs_duration_percentiles,
            requests_total,
            requests_status_codes_total,
            requests_duration_percentiles,
            simulation_peer_requests_per_second,
            simulation_min_peer_requests_per_second,
        }));
        let m = inner.clone();
        meter.register_callback(&instruments, move |observer| {
            let mut metrics = m
                .lock()
                .expect("should be able to acquire metrics lock for reading");
            metrics.observe(observer)
        })?;
        Ok(Self { inner })
    }
    fn record(&mut self, metrics: GooseMetrics, peer_rps: Option<PeerRps>) {
        let mut gm = self
            .inner
            .lock()
            .expect("should be able to acquire metrics lock for mutation");
        gm.goose_metrics = Some(metrics);
        gm.peer_rps = peer_rps;
    }
}

impl MetricsInner {
    fn observe(&mut self, observer: &dyn Observer) {
        if let Some(peer_rps) = &self.peer_rps {
            for info in &peer_rps.0 {
                let mut attrs = self.attrs.clone();
                attrs.push(KeyValue::new("peer", info.name.to_owned()));
                observer.observe_f64(
                    &self.simulation_peer_requests_per_second,
                    info.rps(),
                    &attrs,
                );
            }
            observer.observe_f64(
                &self.simulation_min_peer_requests_per_second,
                peer_rps.min(),
                &self.attrs,
            );
        }
        if let Some(ref metrics) = self.goose_metrics {
            observer.observe_u64(&self.duration, metrics.duration as u64, &self.attrs);
            observer.observe_u64(
                &self.maximum_users,
                metrics.maximum_users as u64,
                &self.attrs,
            );
            observer.observe_u64(&self.users_total, metrics.total_users as u64, &self.attrs);

            for scenario_metrics in &metrics.scenarios {
                // Push and pop unique attributes for each new metric
                self.attrs
                    .push(KeyValue::new("name", scenario_metrics.name.clone()));

                observer.observe_u64(
                    &self.scenarios_total,
                    scenario_metrics.times.count() as u64,
                    &self.attrs,
                );

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    observer.observe_f64(
                        &self.scenarios_duration_percentiles,
                        scenario_metrics.times.quantile(q),
                        &self.attrs,
                    );
                    self.attrs.pop();
                }

                // Pop name
                self.attrs.pop();
            }

            for tx_metrics in metrics.transactions.iter().flatten() {
                // Push and pop unique attributes for each new metric
                self.attrs.push(KeyValue::new(
                    "scenario_name",
                    tx_metrics.scenario_name.clone(),
                ));
                self.attrs.push(KeyValue::new(
                    "tx_name",
                    tx_metrics.transaction_name.clone(),
                ));

                self.attrs.push(KeyValue::new("result", "success"));
                observer.observe_u64(
                    &self.txs_total,
                    tx_metrics.success_count as u64,
                    &self.attrs,
                );
                self.attrs.pop();

                self.attrs.push(KeyValue::new("result", "fail"));
                observer.observe_u64(&self.txs_total, tx_metrics.fail_count as u64, &self.attrs);
                self.attrs.pop();

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    observer.observe_f64(
                        &self.txs_duration_percentiles,
                        tx_metrics.times.quantile(q),
                        &self.attrs,
                    );
                    self.attrs.pop();
                }

                // Pop scenario_name and tx_name
                self.attrs.pop();
                self.attrs.pop();
            }

            for req_metrics in metrics.requests.values() {
                // Push and pop unique attributes for each new metric
                self.attrs
                    .push(KeyValue::new("path", req_metrics.path.clone()));
                self.attrs
                    .push(KeyValue::new("method", format!("{}", req_metrics.method)));

                self.attrs.push(KeyValue::new("result", "success"));
                observer.observe_u64(
                    &self.requests_total,
                    req_metrics.success_count as u64,
                    &self.attrs,
                );
                self.attrs.pop();

                self.attrs.push(KeyValue::new("result", "fail"));
                observer.observe_u64(
                    &self.requests_total,
                    req_metrics.fail_count as u64,
                    &self.attrs,
                );
                self.attrs.pop();

                for (code, count) in &req_metrics.status_code_counts {
                    self.attrs.push(KeyValue::new("code", code.to_string()));
                    observer.observe_u64(
                        &self.requests_status_codes_total,
                        *count as u64,
                        &self.attrs,
                    );
                    self.attrs.pop();
                }

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    observer.observe_f64(
                        &self.requests_duration_percentiles,
                        req_metrics.raw_data.times.quantile(q),
                        &self.attrs,
                    );
                    self.attrs.pop();
                }

                // Pop path and method
                self.attrs.pop();
                self.attrs.pop();
            }
        }
    }
}

#[cfg(test)]
mod test {

    use std::collections::HashMap;

    use keramik_common::peer_info::CeramicPeerInfo;
    use test_log::test;

    use super::*;

    #[derive(Clone, Debug)]
    struct MockMetricsCollector {
        before_counter: u64,
        after_counter: u64,
        host_queries: Arc<Mutex<HashMap<String, u64>>>,
    }

    impl MockMetricsCollector {
        fn new(before_counter: u64, after_counter: u64) -> Self {
            Self {
                before_counter,
                after_counter,
                host_queries: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl MetricsCollection for MockMetricsCollector {
        async fn collect_counter(&self, host: Url, metric_name: &str) -> Result<Option<u64>> {
            let key = format!("{}:{}", host, metric_name);
            let mut lock = self.host_queries.lock().unwrap();
            let value = lock.entry(key).or_insert_with(|| 0);
            let res = match value {
                0 => Some(self.before_counter),
                1 => Some(self.after_counter),
                _ => None,
            };
            *value += 1;
            tracing::debug!(?self.host_queries, "collecting metric {:?} for host {} got {:?}", metric_name, host, res);
            Ok(res)
        }
        fn boxed(&self) -> MetricsCollector {
            Box::new(self.clone())
        }
    }

    fn get_opts(scenario: Scenario, manager: bool, target_request_rate: Option<usize>) -> Opts {
        Opts {
            name: "sim-test".to_string(),
            scenario,
            manager,
            target_peer: 0,
            peers: "/fake/path.json".into(),
            users: 1,
            run_time: "60".into(),
            nonce: 42,
            throttle_requests: None,
            log_level: None,
            target_request_rate,
            // anchor_wait_time: None,
        }
    }

    fn get_peers() -> Vec<Peer> {
        // ceramic addrs must be unique per peer for tests, in practice they are the same
        // we use a map to track which peers have made requests to the metrics endpoint in the mock
        vec![
            Peer::Ceramic(CeramicPeerInfo {
                peer_id: "0".into(),
                ceramic_addr: "http://ceramic-0:7007".into(),
                ipfs_rpc_addr: "http://ipfs-0:5001".into(),
                p2p_addrs: vec!["p2p/p2p-circuit-0/ipfs".into()],
            }),
            Peer::Ceramic(CeramicPeerInfo {
                peer_id: "1".into(),
                ceramic_addr: "http://ceramic-1:7007".into(),
                ipfs_rpc_addr: "http://ipfs-1:5001".into(),
                p2p_addrs: vec!["p2p/p2p-circuit-1/ipfs".into()],
            }),
        ]
    }

    async fn run_event_id_sync_test(
        manager: bool,
        run_time: u64,
        request_cnt: u64,
        target_request_rate: Option<usize>,
        metric_start_value: u64,
        metric_end_value: u64,
    ) -> CommandResult {
        let opts = get_opts(Scenario::ReconEventSync, manager, target_request_rate);

        let peers = get_peers();
        let metrics_collector = MockMetricsCollector::new(metric_start_value, metric_end_value);
        let mut state = ScenarioState::try_from_opts(opts, metrics_collector.boxed(), Some(peers))
            .await
            .unwrap();

        state.collect_before_metrics().await.unwrap();
        state
            .validate_recon_scenario_success_int(run_time, run_time * request_cnt)
            .await
            .0
    }

    #[test(tokio::test)]
    async fn event_id_sync_verify_metrics_exact_default() {
        let run_time = 60;
        let request_cnt = 300;
        let manager = true;
        let target_rps = None; // use default 300
        let metric_start_value = 0;
        let metric_end_value = run_time * request_cnt;
        match run_event_id_sync_test(
            manager,
            run_time,
            request_cnt,
            target_rps,
            metric_start_value,
            metric_end_value,
        )
        .await
        {
            CommandResult::Success => (),
            e => panic!("expected success, got {:?}", e),
        }
    }

    #[test(tokio::test)]
    async fn event_id_sync_verify_metrics_overridden_target() {
        let run_time = 60;
        let request_cnt = 55;
        let manager = true;
        let target_rps = Some(50);
        let metric_start_value = 0;
        let metric_end_value = run_time * request_cnt;
        match run_event_id_sync_test(
            manager,
            run_time,
            request_cnt,
            target_rps,
            metric_start_value,
            metric_end_value,
        )
        .await
        {
            CommandResult::Success => (),
            e => panic!("expected success, got {:?}", e),
        }
    }

    #[test(tokio::test)]
    async fn event_id_sync_verify_metrics_overridden_target_too_low() {
        let run_time = 60;
        let request_cnt = 45;
        let manager = true;
        let target_rps = Some(50);
        let metric_start_value = 0;
        let metric_end_value = run_time * request_cnt;
        match run_event_id_sync_test(
            manager,
            run_time,
            request_cnt,
            target_rps,
            metric_start_value,
            metric_end_value,
        )
        .await
        {
            CommandResult::Failure(e) => {
                info!("got expected failure: {}", e);
            }
            e => panic!("expected failure, got {:?}", e),
        }
    }

    #[test(tokio::test)]
    async fn event_id_sync_verify_metrics_too_low() {
        let run_time = 60;
        let request_cnt = 300;
        let manager = true;
        let target_rps = None; // use default 300
        let metric_start_value = 0;
        let metric_end_value = run_time * request_cnt - 1;
        match run_event_id_sync_test(
            manager,
            run_time,
            request_cnt,
            target_rps,
            metric_start_value,
            metric_end_value,
        )
        .await
        {
            CommandResult::Failure(e) => {
                info!("got expected failure: {}", e);
            }
            e => panic!("expected failure, got {:?}", e),
        }
    }

    #[test(tokio::test)]
    async fn event_id_sync_verify_metrics_too_low_ok_workers() {
        let run_time = 60;
        let request_cnt = 300;
        let manager = false;
        let target_rps = None; // use default 300
        let metric_start_value = 0;
        let metric_end_value = run_time * request_cnt - 1;
        match run_event_id_sync_test(
            manager,
            run_time,
            request_cnt,
            target_rps,
            metric_start_value,
            metric_end_value,
        )
        .await
        {
            CommandResult::Success => (),
            e => panic!("expected success, got {:?}", e),
        }
    }

    #[test(tokio::test)]
    async fn event_id_sync_verify_metrics_too_low_second_run() {
        let run_time = 60;
        let request_cnt = 300;
        let manager = true;
        let target_rps = None; // use default 300
        let metric_start_value = 1; //metrics exist, not first run
        let metric_end_value = run_time * request_cnt;
        match run_event_id_sync_test(
            manager,
            run_time,
            request_cnt,
            target_rps,
            metric_start_value,
            metric_end_value,
        )
        .await
        {
            CommandResult::Failure(e) => {
                info!("got expected failure: {}", e);
            }
            e => panic!("expected failure, got {:?}", e),
        }
    }
}
