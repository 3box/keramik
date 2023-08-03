use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, bail, Result};
use clap::{Args, ValueEnum};
use goose::{config::GooseConfiguration, prelude::GooseMetrics, GooseAttack};
use keramik_common::peer_info::{Peer, PeerIdx};
use opentelemetry::{global, metrics::ObservableGauge, Context, KeyValue};
use tracing::error;

use crate::{
    scenario::{ceramic, ipfs_block_fetch},
    utils::parse_peers_info,
};

/// Options to Simulate command
#[derive(Args, Debug)]
pub struct Opts {
    /// Simulation scenario to run.
    #[arg(long, value_enum, env = "SIMULATE_SCENARIO")]
    scenario: Scenario,

    /// Id of the peer to target.
    #[arg(long, env = "SIMULATE_MANAGER")]
    manager: bool,

    /// Id of the peer to target.
    #[arg(long, env = "SIMULATE_TARGET_PEER")]
    target_peer: usize,

    /// Path to file containing the list of peers.
    /// File should contian JSON encoding of Vec<Peer>.
    #[arg(long, env = "SIMULATE_PEERS_PATH")]
    peers: PathBuf,

    /// Number of users to simulate
    #[arg(long, default_value_t = 100, env = "SIMULATE_USERS")]
    users: usize,

    /// Duration of the simulation
    #[arg(long, env = "SIMULATE_RUN_TIME", default_value = "10m")]
    run_time: String,

    /// Unique value per test run to ensure uniqueness across different test runs.
    /// All workers and manager must be given the same nonce.
    #[arg(long, env = "SIMULATE_NONCE")]
    nonce: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Topology {
    pub target_worker: usize,
    pub total_workers: usize,
    pub nonce: u64,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum Scenario {
    /// Queries the Id of the IPFS peers.
    IpfsRpc,
    /// Simple Ceramic Scenario
    CeramicSimple,
    /// WriteOnly Ceramic Scenario
    CeramicWriteOnly,
    /// New Streams Ceramic Scenario
    CeramicNewStreams,
}

impl Scenario {
    pub fn name(&self) -> &'static str {
        match self {
            Scenario::IpfsRpc => "ipfs_rpc",
            Scenario::CeramicSimple => "ceramic_simple",
            Scenario::CeramicWriteOnly => "ceramic_write_only",
            Scenario::CeramicNewStreams => "ceramic_new_streams",
        }
    }

    fn target_addr(&self, peer: &Peer) -> Result<String> {
        match self {
            Self::IpfsRpc => Ok(peer.ipfs_rpc_addr().to_owned()),
            Self::CeramicSimple | Self::CeramicWriteOnly | Self::CeramicNewStreams => match peer {
                Peer::Ceramic(peer) => Ok(peer.ceramic_addr.clone()),
                Peer::Ipfs(_) => Err(anyhow!(
                    "cannot use non ceramic peer as target for simulation {}",
                    self.name(),
                )),
            },
        }
    }
}

#[tracing::instrument]
pub async fn simulate(opts: Opts) -> Result<()> {
    let mut metrics = Metrics::init(&opts)?;

    let peers: BTreeMap<PeerIdx, Peer> = parse_peers_info(opts.peers)
        .await?
        .into_iter()
        .filter(|(_, peer)| matches!(peer, Peer::Ceramic(_)))
        .collect();

    if opts.manager && opts.users % peers.len() != 0 {
        bail!("number of users {} must be a multiple of the number of peers {}, this ensures we can deterministically identifiy each user", opts.users, peers.len())
    }
    // We assume exactly one worker per peer.
    // This allows us to be deterministic in how each user operates.
    let topo = Topology {
        target_worker: opts.target_peer,
        total_workers: peers.len(),
        nonce: opts.nonce,
    };

    let scenario = match opts.scenario {
        Scenario::IpfsRpc => ipfs_block_fetch::scenario(topo)?,
        Scenario::CeramicSimple => ceramic::scenario()?,
        Scenario::CeramicWriteOnly => ceramic::write_only::scenario()?,
        Scenario::CeramicNewStreams => ceramic::new_streams::scenario()?,
    };
    let config = if opts.manager {
        manager_config(peers.len(), opts.users, opts.run_time)
    } else {
        worker_config(
            opts.scenario
                .target_addr(&peers[&(opts.target_peer as PeerIdx)])?,
        )
    };

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

    metrics.record(goose_metrics);

    Ok(())
}

fn manager_config(count: usize, users: usize, run_time: String) -> GooseConfiguration {
    let mut config = GooseConfiguration::default();
    config.log_level = 2;
    config.users = Some(users);
    config.manager = true;
    config.manager_bind_port = 5115;
    config.expect_workers = Some(count);
    config.startup_time = "10s".to_owned();
    config.run_time = run_time;
    config
}
fn worker_config(target_peer_addr: String) -> GooseConfiguration {
    let mut config = GooseConfiguration::default();
    config.request_log = "request.log".to_owned();
    config.log_level = 2;
    config.worker = true;
    config.host = target_peer_addr;
    // We are leveraging k8s dns search path so we do not have to specify the fully qualified
    // domain name explicitly.
    config.manager_host = "manager.goose".to_owned();
    config.manager_port = 5115;
    config
}

struct Metrics {
    inner: Arc<Mutex<MetricsInner>>,
}
struct MetricsInner {
    goose_metrics: Option<GooseMetrics>,
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

        let inner = Arc::new(Mutex::new(MetricsInner {
            goose_metrics: None,
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
    fn record(&mut self, metrics: GooseMetrics) {
        let mut gm = self
            .inner
            .lock()
            .expect("should be able to acquire metrics lock for mutation");
        gm.goose_metrics = Some(metrics);
    }
}

impl MetricsInner {
    fn observe(&mut self, cx: &Context) {
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
