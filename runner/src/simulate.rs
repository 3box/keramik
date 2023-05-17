use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use clap::{Args, ValueEnum};
use goose::{config::GooseConfiguration, prelude::GooseMetrics, GooseAttack};
use opentelemetry::{global, metrics::ObservableGauge, Context, KeyValue};
use tracing::error;

use crate::{
    scenario::ipfs::ipfs_block_fetch,
    utils::{PeerId, PeerRpcAddr},
};
use crate::scenario::ceramic;

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

    /// Total number of peers in the network
    #[arg(long, env = "SIMULATE_TOTAL_PEERS")]
    total_peers: usize,

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
    /// Creates and updates Ceramic streams.
    CeramicSimpleScenario,
}

impl Scenario {
    pub fn name(&self) -> &'static str {
        match self {
            Scenario::IpfsRpc => "ipfs_rpc",
            Scenario::CeramicSimpleScenario => "ceramic_simple",
        }
    }
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
    requests_total: ObservableGauge<u64>,
    requests_status_codes_total: ObservableGauge<u64>,
    requests_duration_percentiles: ObservableGauge<f64>,
}

impl Metrics {
    fn init(opts: &Opts) -> Self {
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

        //TODO(nathanielc): capture per transaction and scenario metrics

        let inner = Arc::new(Mutex::new(MetricsInner {
            goose_metrics: None,
            attrs,
            duration,
            maximum_users,
            users_total,
            requests_total,
            requests_status_codes_total,
            requests_duration_percentiles,
        }));
        let m = inner.clone();
        meter
            .register_callback(move |cx| {
                let mut metrics = m
                    .lock()
                    .expect("should be able to acquire metrics lock for reading");
                metrics.observe(cx)
            })
            // TODO handle this
            .unwrap();
        Self { inner }
    }
    fn record(&mut self, metrics: GooseMetrics) {
        let mut gm = self
            .inner
            .lock()
            .expect("should be able to acquire metrics lock for mutation");
        (*gm).goose_metrics = Some(metrics);
    }
}

impl MetricsInner {
    fn observe(&mut self, cx: &Context) {
        if let Some(ref metrics) = self.goose_metrics {
            self.duration
                .observe(&cx, metrics.duration as u64, &self.attrs);
            self.maximum_users
                .observe(&cx, metrics.maximum_users as u64, &self.attrs);
            self.users_total
                .observe(&cx, metrics.total_users as u64, &self.attrs);

            for req_metrics in metrics.requests.values() {
                // Push and pop unique attributes for each new metric
                self.attrs
                    .push(KeyValue::new("path", req_metrics.path.clone()));
                self.attrs
                    .push(KeyValue::new("method", format!("{}", req_metrics.method)));

                self.attrs.push(KeyValue::new("result", "success"));
                self.requests_total
                    .observe(&cx, req_metrics.success_count as u64, &self.attrs);
                self.attrs.pop();

                self.attrs.push(KeyValue::new("result", "fail"));
                self.requests_total
                    .observe(&cx, req_metrics.fail_count as u64, &self.attrs);
                self.attrs.pop();

                for (code, count) in &req_metrics.status_code_counts {
                    self.attrs.push(KeyValue::new("code", code.to_string()));
                    self.requests_status_codes_total
                        .observe(&cx, *count as u64, &self.attrs);
                    self.attrs.pop();
                }

                for q in [0.5, 0.75, 0.9, 0.95, 0.99, 0.999] {
                    self.attrs.push(KeyValue::new("percentile", q.to_string()));
                    self.requests_duration_percentiles.observe(
                        &cx,
                        req_metrics.raw_data.histogram.estimate_quantile(q),
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

#[tracing::instrument]
pub async fn simulate(opts: Opts) -> Result<()> {
    let mut metrics = Metrics::init(&opts);

    if opts.users % opts.total_peers != 0 {
        bail!("number of users must be a multiple of the number of peers, this ensures we can deterministically identifiy each user")
    }
    // We assume exactly one worker per peer.
    // This allows us to be deterministic in how each user operates.
    let topo = Topology {
        target_worker: opts.target_peer,
        total_workers: opts.total_peers,
        nonce: opts.nonce,
    };

    let scenario = match opts.scenario {
        Scenario::IpfsRpc => ipfs_block_fetch::scenario(topo)?,
        Scenario::CeramicSimpleScenario => ceramic::scenario(topo)?,
    };
    let config = if opts.manager {
        manager_config(opts.total_peers, opts.users, opts.run_time)
    } else {
        worker_config(opts.target_peer)
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

fn manager_config(total: usize, users: usize, run_time: String) -> GooseConfiguration {
    let mut config = GooseConfiguration::default();
    config.log_level = 2;
    config.users = Some(users);
    // manager requires a host to be set even if its not doing any simulations.
    config.host = PeerRpcAddr::from(0).as_string();
    config.manager = true;
    config.manager_bind_port = 5115;
    config.expect_workers = Some(total);
    config.startup_time = "10s".to_owned();
    config.run_time = run_time;
    config
}
fn worker_config(target_peer: PeerId) -> GooseConfiguration {
    let mut config = GooseConfiguration::default();
    config.log_level = 2;
    config.worker = true;
    config.host = PeerRpcAddr::from(target_peer).as_string();
    config.manager_host = "manager.goose.keramik-0.svc.cluster.local".to_string();
    config.manager_port = 5115;
    config
}
