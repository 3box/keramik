use std::{sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::api::{
    apps::v1::StatefulSet,
    batch::v1::Job,
    core::v1::{ConfigMap, Namespace, Pod, Service},
};

use kube::{
    api::{Patch, PatchParams},
    client::Client,
    core::object::HasSpec,
    runtime::Controller,
    Api,
};
use kube::{
    runtime::{
        controller::Action,
        watcher::{self, Config},
    },
    Resource, ResourceExt,
};

use tracing::{debug, error};

use crate::simulation::{
    manager, manager::ManagerConfig, worker, worker::WorkerConfig, Simulation, SimulationStatus,
};

use crate::monitoring::{jaeger, opentelemetry, prometheus};

use crate::network::{
    controller::PEERS_CONFIG_MAP_NAME,
    peers::PEERS_MAP_KEY,
    utils::{HttpRpcClient, IpfsRpcClient},
    Network,
};

use keramik_common::peer_info::CeramicPeerInfo;

use crate::utils::{
    apply_account, apply_cluster_role, apply_cluster_role_binding, apply_config_map, apply_job,
    apply_service, apply_stateful_set, Context, MANAGED_BY_LABEL_SELECTOR,
};

/// Handle errors during reconciliation.
fn on_error(
    _network: Arc<Simulation>,
    _error: &Error,
    _context: Arc<Context<impl IpfsRpcClient>>,
) -> Action {
    Action::requeue(Duration::from_secs(5))
}

/// Errors produced by the reconcile function.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("App error: {source}")]
    App {
        #[from]
        source: anyhow::Error,
    },
    #[error("Kube error: {source}")]
    Kube {
        #[from]
        source: kube::Error,
    },
}

/// Start a controller for the Simulation CRD.
pub async fn run() {
    let k_client = Client::try_default().await.unwrap();
    let context = Arc::new(Context::new(k_client.clone(), HttpRpcClient));

    // Add api for other resources, ie ceramic nodes
    let networks: Api<Network> = Api::all(k_client.clone());
    let simulations: Api<Simulation> = Api::all(k_client.clone());
    let namespaces: Api<Namespace> = Api::all(k_client.clone());
    let services = Api::<Service>::all(k_client.clone());
    let config_maps = Api::<ConfigMap>::all(k_client.clone());
    let jobs = Api::<Job>::all(k_client.clone());
    let pods = Api::<Pod>::all(k_client.clone());

    Controller::new(simulations.clone(), Config::default())
        .owns(
            networks,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .owns(
            namespaces,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .owns(
            services,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .owns(
            config_maps,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .owns(
            jobs,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .owns(
            pods,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .run(reconcile, on_error, context)
        .for_each(|rec_res| async move {
            match rec_res {
                Ok((simulation, _)) => {
                    debug!(simulation.name, "reconcile success");
                }
                Err(err) => {
                    error!(?err, "reconcile error")
                }
            }
        })
        .await;
}

/// Perform a reconile pass for the Simulation CRD
async fn reconcile(
    simulation: Arc<Simulation>,
    cx: Arc<Context<impl IpfsRpcClient>>,
) -> Result<Action, Error> {
    let spec = simulation.spec();
    debug!(?spec, "reconcile");

    let status = if let Some(status) = &simulation.status {
        status.clone()
    } else {
        SimulationStatus::default()
    };

    let ns = simulation.namespace().unwrap();
    let num_peers = get_num_peers(cx.clone(), &ns).await?;

    apply_jaeger(cx.clone(), &ns, simulation.clone()).await?;
    apply_prometheus(cx.clone(), &ns, simulation.clone()).await?;
    apply_opentelemetry(cx.clone(), &ns, simulation.clone()).await?;

    let ready = monitoring_ready(cx.clone(), &ns).await?;

    if !ready {
        return Ok(Action::requeue(Duration::from_secs(10)));
    }

    let manager_config = ManagerConfig {
        scenario: spec.scenario.to_owned(),
        users: spec.users.to_owned(),
        run_time: spec.run_time.to_owned(),
        nonce: status.nonce,
    };

    apply_manager(cx.clone(), &ns, simulation.clone(), manager_config).await?;

    let jobs: Api<Job> = Api::namespaced(cx.k_client.clone(), &ns);
    let manager_job = jobs.get_status(MANAGER_JOB_NAME).await?;
    let manager_ready = manager_job.status.unwrap().ready.unwrap_or_default();

    if manager_ready > 0 {
        //for loop n peers
        apply_n_workers(cx.clone(), &ns, num_peers, status.nonce, simulation.clone()).await?;
    }

    let simulations: Api<Simulation> = Api::namespaced(cx.k_client.clone(), &ns);
    let _patched = simulations
        .patch_status(
            &simulation.name_any(),
            &PatchParams::default(),
            &Patch::Merge(serde_json::json!({ "status": status })),
        )
        .await?;

    //TODO jobs done/fail cleanup, post process

    Ok(Action::requeue(Duration::from_secs(10)))
}

pub const MANAGER_SERVICE_NAME: &str = "goose";
pub const MANAGER_JOB_NAME: &str = "simulate-manager";
pub const WORKER_JOB_NAME: &str = "simulate-worker";

pub const JAEGER_SERVICE_NAME: &str = "jaeger";
pub const OTEL_SERVICE_NAME: &str = "otel";

pub const OTEL_CR_BINDING: &str = "monitoring-cluster-role-binding";
pub const OTEL_CR: &str = "monitoring-cluster-role";
pub const OTEL_ACCOUNT: &str = "monitoring-service-account";

pub const OTEL_CONFIG_MAP_NAME: &str = "otel-config";
pub const PROM_CONFIG_MAP_NAME: &str = "prom-config";

async fn apply_manager(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    simulation: Arc<Simulation>,
    config: ManagerConfig,
) -> Result<(), kube::error::Error> {
    let orefs = simulation
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap();

    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        MANAGER_SERVICE_NAME,
        manager::service_spec(),
    )
    .await?;
    apply_job(
        cx.clone(),
        ns,
        orefs.clone(),
        MANAGER_JOB_NAME,
        manager::manager_job_spec(config),
    )
    .await?;

    Ok(())
}

async fn get_num_peers(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
) -> Result<u32, kube::error::Error> {
    let config_maps: Api<ConfigMap> = Api::namespaced(cx.k_client.clone(), ns);
    let map = config_maps.get(PEERS_CONFIG_MAP_NAME).await?;
    let data = map.data.unwrap();
    let value = data.get(PEERS_MAP_KEY).unwrap();
    let peers: Vec<CeramicPeerInfo> = serde_json::from_str(value).unwrap();
    Ok(peers.len() as u32)
}

async fn monitoring_ready(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.k_client.clone(), ns);
    let jaeger = stateful_sets.get_status("jaeger").await?;
    let prom = stateful_sets.get_status("prometheus").await?;
    let otel = stateful_sets.get_status("opentelemetry").await?;

    let jaeger_ready = jaeger
        .status
        .map(|status| status.ready_replicas.unwrap_or_default() > 0)
        .unwrap_or_default();
    let prom_ready = prom
        .status
        .map(|status| status.ready_replicas.unwrap_or_default() > 0)
        .unwrap_or_default();
    let otel_ready = otel
        .status
        .map(|status| status.ready_replicas.unwrap_or_default() > 0)
        .unwrap_or_default();

    Ok(jaeger_ready && prom_ready && otel_ready)
}

async fn apply_n_workers(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    peers: u32,
    nonce: u32,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let spec = simulation.spec();
    let orefs = simulation
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap();

    for i in 0..peers {
        let config = WorkerConfig {
            scenario: spec.scenario.to_owned(),
            target_peer: i,
            nonce,
        };

        apply_job(
            cx.clone(),
            ns,
            orefs.clone(),
            &(WORKER_JOB_NAME.to_owned() + "-" + &i.to_string()),
            worker::worker_job_spec(config),
        )
        .await?;
    }

    Ok(())
}

async fn apply_jaeger(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let orefs: Vec<_> = simulation
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap();

    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        JAEGER_SERVICE_NAME,
        jaeger::service_spec(),
    )
    .await?;

    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "jaeger",
        jaeger::stateful_set_spec(),
    )
    .await?;
    Ok(())
}

async fn apply_prometheus(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let orefs = simulation
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap();

    apply_config_map(
        cx.clone(),
        ns,
        orefs.clone(),
        PROM_CONFIG_MAP_NAME,
        prometheus::config_map_data(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "prometheus",
        prometheus::stateful_set_spec(),
    )
    .await?;
    Ok(())
}

async fn apply_opentelemetry(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let orefs = simulation
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap();

    apply_account(cx.clone(), ns, orefs.clone(), OTEL_ACCOUNT).await?;
    apply_cluster_role(
        cx.clone(),
        ns,
        orefs.clone(),
        OTEL_CR,
        opentelemetry::cluster_role(),
    )
    .await?;
    apply_cluster_role_binding(
        cx.clone(),
        orefs.clone(),
        OTEL_CR_BINDING,
        opentelemetry::cluster_role_binding(ns),
    )
    .await?;
    apply_config_map(
        cx.clone(),
        ns,
        orefs.clone(),
        OTEL_CONFIG_MAP_NAME,
        opentelemetry::config_map_data(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        OTEL_SERVICE_NAME,
        opentelemetry::service_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "opentelemetry",
        opentelemetry::stateful_set_spec(),
    )
    .await?;

    Ok(())
}
