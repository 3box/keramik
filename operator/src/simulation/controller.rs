#![allow(dead_code)]
#![allow(unused_variables)]
use std::{ sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::{
    api::{
        batch::v1::Job,
        core::v1::{ConfigMap, Namespace, Pod, Service },
    },
};

use kube::{
    // api::{ Patch, PatchParams},
    client::Client,
    core::{object::HasSpec },
    runtime::Controller,
    Api,
};
use kube::{
    runtime::{
        controller::Action,
        watcher::{self, Config},
    },
    Resource,
};

use tracing::{debug, error };

use crate::simulation::{ 
    manager, worker, Simulation, manager::ManagerConfig, worker::WorkerConfig
};

use crate::opentelemetry::{opentelemetry, jaeger, prometheus };

use crate::network::{ Network, NetworkStatus};
use crate::utils::{ 
    apply_job, apply_service, apply_stateful_set, apply_account, apply_cluster_role, 
    apply_cluster_role_binding, apply_config_map, ContextData, MANAGED_BY_LABEL_SELECTOR
};

/// Handle errors during reconciliation.
fn on_error(_network: Arc<Simulation>, _error: &Error, _context: Arc<ContextData>) -> Action {
    Action::requeue(Duration::from_secs(5))
}

// MOVE TO UTILS
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
    let k_client: Client = Client::try_default().await.unwrap();
    let context: Arc<ContextData> = Arc::new(ContextData::new(k_client.clone()));

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
                Ok((network, _)) => {
                    debug!(network.name, "reconile success");
                }
                Err(err) => {
                    error!(?err, "reconile error")
                }
            }
        })
        .await;
}

/// Perform a reconile pass for the Simulation CRD
async fn reconcile(simulation: Arc<Simulation>, cx: Arc<ContextData>) -> Result<Action, Error> {
    let spec = simulation.spec();
    debug!(?spec, "reconcile");

    // let mut status = if let Some(status) = &simulation.status {
    //     status.clone()
    // } else {
    //     SimulationStatus::default()
    // };

    let sim_name = &spec.selector;
    let net_status = get_network_status(cx.clone(), sim_name).await?;
    //TODO handle err not found, no matching network

    let net_ready = net_status.ready_replicas == net_status.replicas;

    if !net_ready {
        debug!("simulation waiting, network not ready");
        return  Ok(Action::requeue(Duration::from_secs(10)))
    }

    let num_peers = net_status.ready_replicas;
    let ns = "keramik-".to_owned() + sim_name;


    apply_jaeger(cx.clone(), &ns, simulation.clone()).await?;
    apply_prometheus(cx.clone(), &ns, simulation.clone()).await?;
    apply_opentelemetry(cx.clone(), &ns, simulation.clone()).await?;

    // add into from sim spec 
    // let config = ManagerSpec {
    //     scenario: Some(spec.scenario.to_owned()),
    //     total_peers: Some(num_peers),
    //     users: Some(spec.users.to_owned()),
    //     run_time: Some(spec.run_time.to_owned()),
    //     nonce: None
    // };

    let config = ManagerConfig {
        scenario: "ipfs-rpc".to_owned(),
        // TODO
        nonce: 1,
        total_peers: 1,
        users: 100,
        run_time: 10,
    };

    apply_manager(
        cx.clone(),
        &ns,
        simulation.clone(),
        config.into(),
    )
    .await?;


    // assume there is a ready event as well that could be used
    let jobs: Api<Job> = Api::namespaced(cx.client.clone(), &ns);
    let manager_job = jobs.get_status(MANAGER_JOB_NAME).await?;
    // why not ?, compiler
    let manager_ready = manager_job.status.unwrap().ready.unwrap();

    if manager_ready > 0 {
        //for loop n peers 
        apply_n_workers(
            cx.clone(),
            &ns,
            num_peers as u32,
            simulation.clone(),
          )
        .await?;
    }

    // TODO, simulation status
    // let networks: Api<Network> = Api::all(cx.client.clone());
    // let _patched = networks
    //     .patch_status(
    //         &network.name_any(),
    //         &PatchParams::default(),
    //         &Patch::Merge(serde_json::json!({ "status": status })),
    //     )
    //     .await?;

    //TODO jobs done/fail cleanup, post process

    Ok(Action::requeue(Duration::from_secs(10)))
}

pub const MANAGER_SERVICE_NAME: &str = "sim-manager";
pub const MANAGER_JOB_NAME: &str = "sim-manager-job";
pub const WORKER_JOB_NAME: &str = "worker-job";

pub const JAEGER_SERVICE_NAME: &str = "jaeger";
pub const OTEL_SERVICE_NAME: &str = "otel";

pub const OTEL_CR_BINDING: &str = "monitoring-cluster-role-binding";
pub const OTEL_CR: &str = "monitoring-cluster-role";
pub const OTEL_ACCOUNT: &str = "monitoring-service-account";

pub const OTEL_CONFIG_MAP_NAME: &str = "otel-config";
pub const PROM_CONFIG_MAP_NAME: &str = "prom-config";

async fn apply_manager(
    cx: Arc<ContextData>,
    ns: &str,
    simulation: Arc<Simulation>,
    config: ManagerConfig
) -> Result<(), kube::error::Error> {
    let oref_sim = simulation.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref_sim];

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
        manager::manager_job_spec(config)
    ).await?;

    Ok(())
}

async fn get_network_status(
    cx: Arc<ContextData>,
    name: &str,
) -> Result<NetworkStatus, kube::error::Error> {
    let network: Api<Network> = Api::all(cx.client.clone());
    let net = network.get_status(name).await?;
    Ok(net.status.unwrap())
}

async fn apply_n_workers(
    cx: Arc<ContextData>,
    ns: &str,
    n: u32,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let spec = simulation.spec();
    let oref_sim = simulation.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref_sim];

    for i in 0..n {
        let config = WorkerConfig {
            scenario: spec.scenario.to_owned(),
            target_peer: i,
            total_peers: n,
            // TODO, what does nonce do 
            nonce: 1,
        };

        apply_job(
            cx.clone(),
            &ns,
            orefs.clone(),
            &(WORKER_JOB_NAME.to_owned() + "-" + &i.to_string()),
            worker::worker_job_spec(config),
        )
        .await?;
    }

    Ok(())
}


async fn apply_jaeger(
    cx: Arc<ContextData>,
    ns: &str,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let oref_sim = simulation.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref_sim];

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
    cx: Arc<ContextData>,
    ns: &str,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let oref_sim = simulation.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref_sim];

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
    cx: Arc<ContextData>,
    ns: &str,
    simulation: Arc<Simulation>,
) -> Result<(), kube::error::Error> {
    let oref_sim = simulation.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref_sim];

    apply_account(
        cx.clone(), 
        ns, 
        orefs.clone(), 
        OTEL_ACCOUNT
    )
    .await?;
    apply_cluster_role(
        cx.clone(), 
        ns, 
        orefs.clone(), 
        OTEL_CR, 
        opentelemetry::cluster_role()
    )
    .await?;
    apply_cluster_role_binding(
        cx.clone(), 
        ns, 
        orefs.clone(), 
        OTEL_CR_BINDING, 
        opentelemetry::cluster_role_binding()
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