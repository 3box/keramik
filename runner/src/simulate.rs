use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use clap::{Args, ValueEnum};
use goose::{config::GooseConfiguration, prelude::GooseMetrics, GooseAttack};
use keramik_common::peer_info::Peer;
use opentelemetry::{global, metrics::ObservableGauge, Context, KeyValue};
use reqwest::Url;
use tracing::{error, info, warn};

use crate::{
    scenario::{ceramic, ipfs_block_fetch},
    utils::parse_peers_info,
    CommandResult,
};

// FIXME: is it worth attaching metrics to the peer info?
const IPFS_SERVICE_METRICS_PORT: u32 = 9465;
const EVENT_SYNC_METRIC_NAME: &str = "recon_key_insert_count_total";

/// Options to Simulate command
#[derive(Args, Debug)]
pub struct Opts {
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
}

#[derive(Debug, Clone, Copy)]
pub struct Topology {
    pub target_worker: usize,
    pub total_workers: usize,
    pub users: usize,
    pub nonce: u64,
}

#[derive(Clone, Debug, Copy, ValueEnum)]
pub enum Scenario {
    /// Queries the Id of the IPFS peers.
    IpfsRpc,
    /// Simple Ceramic Scenario
    CeramicSimple,
    /// WriteOnly Ceramic Scenario
    CeramicWriteOnly,
    /// New Streams Ceramic Scenario
    CeramicNewStreams,
    /// Simple Query Scenario
    CeramicQuery,
    /// Scenario to reuse the same model id and query instances across workers
    CeramicModelReuse,
    /// Nodes subscribe to same model. One node generates new events, recon syncs event keys and data to peers.
    ReconEventSync,
    /// Nodes subscribe to same model. One node generates new events, recon syncs event keys to peers.
    /// Which of the Recon scenarios  you should choose is dictated by the API of the ceramic-one instance
    /// being used. Previously, it only supported keys but newer versions support keys and data.
    /// This scenario is for the keys only version and will fail on new verisons.
    ReconEventKeySync,
}

impl Scenario {
    pub fn name(&self) -> &'static str {
        match self {
            Scenario::IpfsRpc => "ipfs_rpc",
            Scenario::CeramicSimple => "ceramic_simple",
            Scenario::CeramicWriteOnly => "ceramic_write_only",
            Scenario::CeramicNewStreams => "ceramic_new_streams",
            Scenario::CeramicQuery => "ceramic_query",
            Scenario::CeramicModelReuse => "ceramic_model_reuse",
            Scenario::ReconEventSync => "recon_event_sync",
            Scenario::ReconEventKeySync => "recon_event_key_sync",
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
            | Self::CeramicQuery
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
    before_metrics: Option<Vec<u64>>,
    run_time: String,
    throttle_requests: Option<usize>,
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
            throttle_requests: opts.throttle_requests,
        })
    }

    async fn build_goose_scenario(&mut self) -> Result<goose::prelude::Scenario> {
        let scenario = match self.scenario {
            Scenario::IpfsRpc => ipfs_block_fetch::scenario(self.topo)?,
            Scenario::CeramicSimple => ceramic::simple::scenario().await?,
            Scenario::CeramicWriteOnly => ceramic::write_only::scenario().await?,
            Scenario::CeramicNewStreams => ceramic::new_streams::scenario().await?,
            Scenario::CeramicQuery => ceramic::query::scenario().await?,
            Scenario::CeramicModelReuse => ceramic::model_reuse::scenario().await?,
            Scenario::ReconEventSync => ceramic::recon_sync::event_sync_scenario().await?,
            Scenario::ReconEventKeySync => ceramic::recon_sync::event_key_sync_scenario().await?,
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
        metrics_port: u32,
    ) -> Result<Vec<Option<u64>>> {
        // This is naive and specific to our requirement of getting a prometheus counter.
        let mut results = Vec::with_capacity(self.peers.len());
        for peer in self.peers.iter() {
            // Ideally, the peer stores the available metrics endpoints and we don't need to build them,
            // but that's not the case right now and the ceramic/recon split makes it a bit odd, as most
            // scenarios care about the ceramic metrics, but the recon metrics are on the IPFS port.
            if let Some(addr) = peer.ceramic_addr() {
                let addr = addr.parse::<reqwest::Url>()?;
                if let Some(host) = addr.host_str() {
                    let url = format!("http://{}:{}", host, metrics_port);
                    let metric = self
                        .metrics_collector
                        .collect_counter(url.parse()?, metric_name)
                        .await?;
                    results.push(metric);
                } else {
                    warn!("Failed to parse ceramic addr for host: {}", addr);
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
                | Scenario::CeramicModelReuse => Ok(()),
                Scenario::ReconEventSync | Scenario::ReconEventKeySync => {
                    let peers = self
                        .get_peers_counter_metric(EVENT_SYNC_METRIC_NAME, IPFS_SERVICE_METRICS_PORT)
                        .await?;
                    let res: Vec<u64> = peers.iter().filter_map(|v| *v).collect();
                    if res.len() != peers.len() {
                        bail!(
                            "Failed to collect metrics for all peers before scenario {:?}: {:?}",
                            self.scenario,
                            peers
                        )
                    }
                    self.before_metrics = Some(res);
                    Ok(())
                }
            }
        }
    }

    /// For now, most scenarios are successful if they complete without error and only EventIdSync has a criteria.
    /// Not a result to ensure we always proceed with cleanup, even if we fail to validate the scenario.
    pub async fn validate_scenario_success(
        &self,
        metrics: &GooseMetrics,
    ) -> (CommandResult, Option<f64>) {
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
            Scenario::ReconEventSync | Scenario::ReconEventKeySync => {
                // It'd be easy to make work for other scenarios if they defined a rate and metric. However, the scenario we're
                // interested in is asymmetrical in what the workers do, and we're trying to look at what happens to other nodes,
                // which is not how most scenarios work. It also uses the IPFS metrics endpoint. We could parameterize or use a
                // trait, but we don't yet have a use case, and might need to use transactions, or multiple requests, or something
                // entirely different. Anyway, to avoid generalizing the exception we keep it simple.
                let req_name = ceramic::recon_sync::CREATE_EVENT_REQ_NAME;

                let metric = match metrics
                    .requests
                    .get(req_name)
                    .ok_or_else(|| anyhow!("failed to find goose metrics for request {}", req_name))
                    .map_err(CommandResult::Failure)
                {
                    Ok(v) => v,
                    Err(e) => return (e, None),
                };

                self.validate_scenario_success_int(
                    metrics.duration as u64,
                    metric.success_count as u64,
                )
                .await
            }
        }
    }

    /// Removed from `validate_scenario_success` to make testing easier as it's hard to
    async fn validate_scenario_success_int(
        &self,
        run_time_seconds: u64,
        request_cnt: u64,
    ) -> (CommandResult, Option<f64>) {
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
                let peer_metrics = match peer_req_cnts
                    .into_iter()
                    .zip(before_metrics.iter())
                    .enumerate()
                    .map(|(idx, (current, before))| {
                        if let Some(c) = current {
                            Ok((c, *before, (c - *before) as f64 / run_time_seconds as f64))
                        } else {
                            Err(anyhow!("Peer {} missing metric data", idx))
                        }
                    })
                    .collect::<Result<Vec<(u64, u64, f64)>, anyhow::Error>>()
                {
                    Ok(rps) => rps,
                    Err(err) => return (CommandResult::Failure(err), None),
                };

                let min_peer_rps = peer_metrics.iter().fold(f64::INFINITY, |a, &b| a.min(b.2));

                let mut errors = peer_metrics.into_iter().enumerate().flat_map(|(idx,(req_cnt, before, rps))|
                    if rps < threshold {
                        warn!(current_req_cnt=%req_cnt, %before, %run_time_seconds, %threshold, %rps, "rps less than threshold");
                        Some(
                            format!(
                                "Peer {} RPS less than threshold: {} < {}",
                                idx, rps, threshold
                            ),
                        )
                    } else {
                        info!(current_req_cnt=%req_cnt, %before, %run_time_seconds, %threshold, %rps, "success! peer {} over the threshold", idx);
                        None
                    }
                ).collect::<Vec<String>>();

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
                    (CommandResult::Success, Some(min_peer_rps))
                } else {
                    warn!(?errors, "FAILURE! Not all peers met the threshold");
                    (CommandResult::Failure(anyhow!(errors.join("\n"))), None)
                }
            }
        }
    }

    fn goose_config(&self) -> Result<GooseConfiguration> {
        let config = if self.manager {
            let mut config = GooseConfiguration::default();
            config.log_level = 2;
            config.users = Some(self.topo.users);
            config.manager = true;
            config.manager_bind_port = 5115;
            config.expect_workers = Some(self.topo.total_workers);
            config.startup_time = "10s".to_owned();
            config.run_time = self.run_time.clone();
            config
        } else {
            let mut config = GooseConfiguration::default();
            config.scenario_log = "scenario.log".to_owned();
            config.transaction_log = "transaction.log".to_owned();
            config.request_log = "request.log".to_owned();
            config.error_log = "error.log".to_owned();
            config.log_level = 2;
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

    let (success, min_peer_rps) = state.validate_scenario_success(&goose_metrics).await;
    metrics.record(goose_metrics, min_peer_rps);

    Ok(success)
}

struct Metrics {
    inner: Arc<Mutex<MetricsInner>>,
}
struct MetricsInner {
    goose_metrics: Option<GooseMetrics>,
    min_peer_rps: Option<f64>,

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
}

impl Metrics {
    fn init(opts: &Opts) -> Result<Self> {
        let mut attrs = vec![
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

        let inner = Arc::new(Mutex::new(MetricsInner {
            goose_metrics: None,
            min_peer_rps: None,
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
            simulation_min_peer_requests_per_second,
        }));
        let m = inner.clone();
        meter.register_callback(move |cx| {
            let mut metrics = m
                .lock()
                .expect("should be able to acquire metrics lock for reading");
            metrics.observe(cx)
        })?;
        Ok(Self { inner })
    }
    fn record(&mut self, metrics: GooseMetrics, min_peer_rps: Option<f64>) {
        let mut gm = self
            .inner
            .lock()
            .expect("should be able to acquire metrics lock for mutation");
        gm.goose_metrics = Some(metrics);
        gm.min_peer_rps = min_peer_rps;
    }
}

impl MetricsInner {
    fn observe(&mut self, cx: &Context) {
        // TODO add simulation specific attributes
        if let Some(min_peer_rps) = self.min_peer_rps {
            self.simulation_min_peer_requests_per_second
                .observe(cx, min_peer_rps, &[]);
        }
        if let Some(ref metrics) = self.goose_metrics {
            self.duration
                .observe(cx, metrics.duration as u64, &self.attrs);
            self.maximum_users
                .observe(cx, metrics.maximum_users as u64, &self.attrs);
            self.users_total
                .observe(cx, metrics.total_users as u64, &self.attrs);

            for scenario_metrics in &metrics.scenarios {
                // Push and pop unique attributes for each new metric
                self.attrs
                    .push(KeyValue::new("name", scenario_metrics.name.clone()));

                self.scenarios_total.observe(
                    cx,
                    scenario_metrics.times.count() as u64,
                    &self.attrs,
                );

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    self.scenarios_duration_percentiles.observe(
                        cx,
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
                self.txs_total
                    .observe(cx, tx_metrics.success_count as u64, &self.attrs);
                self.attrs.pop();

                self.attrs.push(KeyValue::new("result", "fail"));
                self.txs_total
                    .observe(cx, tx_metrics.fail_count as u64, &self.attrs);
                self.attrs.pop();

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    self.txs_duration_percentiles.observe(
                        cx,
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
                self.requests_total
                    .observe(cx, req_metrics.success_count as u64, &self.attrs);
                self.attrs.pop();

                self.attrs.push(KeyValue::new("result", "fail"));
                self.requests_total
                    .observe(cx, req_metrics.fail_count as u64, &self.attrs);
                self.attrs.pop();

                for (code, count) in &req_metrics.status_code_counts {
                    self.attrs.push(KeyValue::new("code", code.to_string()));
                    self.requests_status_codes_total
                        .observe(cx, *count as u64, &self.attrs);
                    self.attrs.pop();
                }

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    self.requests_duration_percentiles.observe(
                        cx,
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
            scenario,
            manager,
            target_peer: 0,
            peers: "/fake/path.json".into(),
            users: 1,
            run_time: "60".into(),
            nonce: 42,
            throttle_requests: None,
            target_request_rate,
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
            .validate_scenario_success_int(run_time, run_time * request_cnt)
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
