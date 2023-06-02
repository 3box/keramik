use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetStatus},
        batch::v1::{ Job },
        core::v1::{ConfigMap, Namespace, Pod, Secret, Service, ServiceStatus},
    },
    apimachinery::pkg::util::intstr::IntOrString,
};
use keramik_common::peer_info::PeerInfo;
use kube::{
    api::{DeleteParams, Patch, PatchParams},
    client::Client,
    core::{object::HasSpec, ObjectMeta},
    runtime::Controller,
    Api, ResourceExt,
};
use kube::{
    runtime::{
        controller::Action,
        watcher::{self, Config},
    },
    Resource,
};
use rand::RngCore;
use tracing::{debug, error, trace};

use crate::network::{
    bootstrap, cas,
    ceramic::{self, CeramicConfig},
    peers, utils, BootstrapSpec, CeramicSpec, Network, NetworkStatus,
};

use crate::utils::{ apply_job, apply_service, apply_config_map, apply_stateful_set, ContextData, MANAGED_BY_LABEL_SELECTOR, managed_labels};

/// Handle errors during reconciliation.
fn on_error(_network: Arc<Network>, _error: &Error, _context: Arc<ContextData>) -> Action {
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

/// Start a controller for the Network CRD.
pub async fn run() {
    let k_client: Client = Client::try_default().await.unwrap();
    let context: Arc<ContextData> = Arc::new(ContextData::new(k_client.clone()));

    // Add api for other resources, ie ceramic nodes
    let networks: Api<Network> = Api::all(k_client.clone());
    let namespaces: Api<Namespace> = Api::all(k_client.clone());
    let statefulsets = Api::<StatefulSet>::all(k_client.clone());
    let services = Api::<Service>::all(k_client.clone());
    let config_maps = Api::<ConfigMap>::all(k_client.clone());
    let secrets = Api::<Secret>::all(k_client.clone());
    let jobs = Api::<Job>::all(k_client.clone());
    let pods = Api::<Pod>::all(k_client.clone());

    Controller::new(networks.clone(), Config::default())
        .owns(
            namespaces,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .owns(
            statefulsets,
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
            secrets,
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

// A list of constants used in various K8s resources.
//
// When should you use a constant vs just hardcode the string literal directly?
//
// K8s uses lots of names to relate various resources (i.e. port names).
// We DO NOT want to use a constant for all of those use cases. However if a value
// spans multiple resources then we should create a constant for it.

pub const CONTROLLER_NAME: &str = "keramik";
pub const CERAMIC_STATEFUL_SET_NAME: &str = "ceramic";
pub const CERAMIC_SERVICE_NAME: &str = "ceramic";
pub const CERAMIC_SERVICE_RPC_PORT: i32 = 5001;

pub const INIT_CONFIG_MAP_NAME: &str = "ceramic-init";
pub const PEERS_CONFIG_MAP_NAME: &str = "keramik-peers";
pub const ADMIN_SECRET_NAME: &str = "ceramic-admin";

pub const CAS_SERVICE_NAME: &str = "cas";
pub const CAS_IFPS_SERVICE_NAME: &str = "cas-ipfs";
pub const CAS_POSTGRES_SERVICE_NAME: &str = "cas-postgres";
pub const CAS_POSTGRES_SECRET_NAME: &str = "postgres-auth";
pub const GANACHE_SERVICE_NAME: &str = "ganache";

pub const CERAMIC_APP: &str = "ceramic";
pub const CAS_APP: &str = "cas";
pub const CAS_POSTGRES_APP: &str = "cas-postgres";
pub const CAS_IPFS_APP: &str = "cas-ipfs";
pub const GANACHE_APP: &str = "ganache";

pub const BOOTSTRAP_JOB_NAME: &str = "bootstrap";

/// Perform a reconile pass for the Network CRD
async fn reconcile(network: Arc<Network>, cx: Arc<ContextData>) -> Result<Action, Error> {
    let spec = network.spec();
    debug!(?spec, "reconcile");

    let mut status = if let Some(status) = &network.status {
        status.clone()
    } else {
        NetworkStatus::default()
    };

    let ns = apply_network_namespace(cx.clone(), network.clone()).await?;

    apply_cas(cx.clone(), &ns, network.clone()).await?;

    if is_admin_secret_missing(cx.clone(), &ns).await? {
        create_admin_secret(cx.clone(), &ns, network.clone()).await?;
    }

    apply_ceramic(
        cx.clone(),
        &ns,
        network.clone(),
        spec.replicas,
        spec.ceramic.clone(),
    )
    .await?;

    if status.replicas != spec.replicas {
        if spec.replicas > status.replicas {
            // We will have new peers, delete the bootstrap job
            // so that it will run again once all the replicas are ready.
            delete_job(cx.clone(), &ns, BOOTSTRAP_JOB_NAME).await?;
        } else {
            // We have fewer peers, remove the extra peers from the list
            status.peers.retain(|peer| peer.index < spec.replicas);
        }

        // The number of replicas changed.
        status.replicas = spec.replicas;
    }

    update_peer_info(cx.clone(), &ns, network.clone(), &mut status).await?;

    apply_bootstrap_job(
        cx.clone(),
        &ns,
        network.clone(),
        &status,
        spec.replicas,
        spec.bootstrap.clone(),
    )
    .await?;

    let networks: Api<Network> = Api::all(cx.client.clone());
    let _patched = networks
        .patch_status(
            &network.name_any(),
            &PatchParams::default(),
            &Patch::Merge(serde_json::json!({ "status": status })),
        )
        .await?;

    Ok(Action::requeue(Duration::from_secs(30)))
}

// Applies the namespace
async fn apply_network_namespace(
    cx: Arc<ContextData>,
    network: Arc<Network>,
) -> Result<String, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let namespaces: Api<Namespace> = Api::all(cx.client.clone());
    let ns = "keramik-".to_owned() + &network.name_any();
    let oref = network.controller_owner_ref(&()).unwrap();
    let namespace_data: Namespace = Namespace {
        metadata: ObjectMeta {
            name: Some(ns.clone()),
            owner_references: Some(vec![oref]),
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        ..Default::default()
    };
    namespaces
        .patch(&ns, &serverside, &Patch::Apply(namespace_data))
        .await?;

    Ok(ns)
}

async fn apply_cas(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
) -> Result<(), kube::error::Error> {
    if is_cas_postgres_secret_missing(cx.clone(), ns).await? {
        create_cas_postgres_secret(cx.clone(), ns, network.clone()).await?;
    }
    let oref = network.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref];

    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        CAS_SERVICE_NAME,
        cas::cas_service_spec(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        CAS_IFPS_SERVICE_NAME,
        cas::cas_ipfs_service_spec(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        GANACHE_SERVICE_NAME,
        cas::ganache_service_spec(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        CAS_POSTGRES_SERVICE_NAME,
        cas::postgres_service_spec(),
    )
    .await?;

    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "cas",
        cas::cas_stateful_set_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "cas-ipfs",
        cas::cas_ipfs_stateful_set_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "ganache",
        cas::ganache_stateful_set_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "cas-postgres",
        cas::postgres_stateful_set_spec(),
    )
    .await?;

    Ok(())
}

async fn is_cas_postgres_secret_missing(
    cx: Arc<ContextData>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, CAS_POSTGRES_SECRET_NAME).await
}
async fn create_cas_postgres_secret(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
) -> Result<(), kube::error::Error> {
    // Create postgres_secret
    let mut secret_bytes: Vec<u8> = Vec::new();
    secret_bytes.resize(20, 0);
    rand::thread_rng().fill_bytes(&mut secret_bytes);

    let string_data = BTreeMap::from_iter(vec![
        ("username".to_owned(), "ceramic".to_owned()),
        ("password".to_owned(), hex::encode(secret_bytes)),
    ]);
    create_secret(cx, ns, network, CAS_POSTGRES_SECRET_NAME, string_data).await?;
    Ok(())
}

async fn is_admin_secret_missing(
    cx: Arc<ContextData>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, ADMIN_SECRET_NAME).await
}
async fn create_admin_secret(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
) -> Result<(), kube::error::Error> {
    let mut secret_bytes: Vec<u8> = Vec::new();
    secret_bytes.resize(32, 0);
    rand::thread_rng().fill_bytes(&mut secret_bytes);

    let string_data =
        BTreeMap::from_iter(vec![("private-key".to_owned(), hex::encode(secret_bytes))]);
    create_secret(cx, ns, network, ADMIN_SECRET_NAME, string_data).await?;
    Ok(())
}
async fn apply_ceramic(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
    replicas: i32,
    spec: Option<CeramicSpec>,
) -> Result<(), kube::error::Error> {
    let config: CeramicConfig = spec.into();

    if config.init_config_map == INIT_CONFIG_MAP_NAME {
        //Only create the config map if its the one we own.
        let oref = network.controller_owner_ref(&()).unwrap();
        let orefs = vec![oref];
        apply_config_map(cx.clone(), ns, orefs.clone(), &config.init_config_map, ceramic::init_config_map_data()).await?;
    }

    apply_ceramic_service(cx.clone(), ns, network.clone()).await?;
    apply_ceramic_stateful_set(cx.clone(), ns, network.clone(), replicas, config).await?;

    Ok(())
}

async fn apply_ceramic_service(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    let oref = network.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref];
    apply_service(
        cx,
        ns,
        orefs.clone(),
        CERAMIC_SERVICE_NAME,
        ceramic::service_spec(),
    )
    .await
}

async fn apply_ceramic_stateful_set(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
    replicas: i32,
    config: CeramicConfig,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let spec = ceramic::stateful_set_spec(replicas, config);
    let oref = network.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref];
    apply_stateful_set(cx, ns, orefs, CERAMIC_STATEFUL_SET_NAME, spec).await
}

async fn apply_bootstrap_job(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
    status: &NetworkStatus,
    replicas: i32,
    spec: Option<BootstrapSpec>,
) -> Result<(), Error> {
    // Should we bootstrap?
    let percent = if let Some(BootstrapSpec {
        percent: Some(percent),
        ..
    }) = &spec
    {
        match percent {
            IntOrString::Int(i) => *i as f64 / 100.0,
            IntOrString::String(p) => {
                let percent: &str = if let Some(precent) = &p.strip_suffix('%') {
                    precent
                } else {
                    p
                };
                percent.parse::<f64>().map_err(anyhow::Error::from)? / 100.0
            }
        }
    } else {
        // Default to 100%
        1.0
    };
    let ready = status.peers.len();
    if ready as f64 >= replicas as f64 * percent {
        debug!("creating bootstrap job");
        // Create bootstrap jobs
        let spec = bootstrap::bootstrap_job_spec(spec);
        let oref = network.controller_owner_ref(&()).unwrap();
        let orefs = vec![oref];
        apply_job(cx.clone(), ns, orefs.clone(), BOOTSTRAP_JOB_NAME, spec).await?;
    }
    Ok(())
}

async fn update_peer_info(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
    status: &mut NetworkStatus,
) -> Result<(), Error> {
    for index in 0..status.replicas {
        if status.peers.iter().any(|info| info.index == index) {
            // Skip peers we already know
            continue;
        }
        let (peer_id, rpc_addr, p2p_addrs) = match utils::peer_addr(ns, index).await {
            Ok(res) => res,
            Err(err) => {
                trace!(%err, index, "failed to get peer id and mulitaddrs for peer");
                continue;
            }
        };
        status.peers.push(PeerInfo {
            index,
            peer_id,
            rpc_addr,
            p2p_addrs,
        });
    }
    status.ready_replicas = status.peers.len() as i32;

    let oref = network.controller_owner_ref(&()).unwrap();
    let orefs = vec![oref];
    apply_config_map(
        cx,
        ns,
        orefs,
        PEERS_CONFIG_MAP_NAME,
        peers::peer_config_map_data(&status.peers),
    )
    .await?;
    Ok(())
}



async fn is_secret_missing(
    cx: Arc<ContextData>,
    ns: &str,
    name: &str,
) -> Result<bool, kube::error::Error> {
    let secrets: Api<Secret> = Api::namespaced(cx.client.clone(), ns);
    Ok(secrets.get_opt(name).await?.is_none())
}
async fn create_secret(
    cx: Arc<ContextData>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
    string_data: BTreeMap<String, String>,
) -> Result<(), kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let secrets: Api<Secret> = Api::namespaced(cx.client.clone(), ns);

    let oref = network.controller_owner_ref(&()).unwrap();
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(vec![oref]),
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        string_data: Some(string_data),
        ..Default::default()
    };
    let _secret = secrets
        .patch(name, &serverside, &Patch::Apply(secret))
        .await?;

    Ok(())
}

// Deletes a job. Does nothing if the job does not exist.
async fn delete_job(cx: Arc<ContextData>, ns: &str, name: &str) -> Result<(), kube::error::Error> {
    let jobs: Api<Job> = Api::namespaced(cx.client.clone(), ns);
    if jobs.get_opt(name).await?.is_some() {
        jobs.delete(name, &DeleteParams::default()).await?;
    }
    Ok(())
}