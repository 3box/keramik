use std::{cmp::min, collections::BTreeMap, str::from_utf8, sync::Arc, time::Duration};

use anyhow::anyhow;
use futures::stream::StreamExt;
use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetStatus},
        batch::v1::Job,
        core::v1::{ConfigMap, Namespace, Pod, Secret, Service, ServiceStatus},
    },
    apimachinery::pkg::apis::meta::v1::Time,
};
use keramik_common::peer_info::{CeramicPeerInfo, Peer};
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
use tracing::{debug, error, info, trace, warn};

use crate::{
    labels::{managed_labels, MANAGED_BY_LABEL_SELECTOR},
    network::{
        bootstrap::{self, BootstrapConfig},
        cas,
        ceramic::{self, CeramicBundle, CeramicConfigs, CeramicInfo, NetworkConfig},
        datadog::DataDogConfig,
        ipfs_rpc::{HttpRpcClient, IpfsRpcClient},
        peers, CasSpec, Network, NetworkStatus,
    },
    utils::Clock,
    CONTROLLER_NAME,
};

use crate::utils::{
    apply_config_map, apply_job, apply_service, apply_stateful_set, delete_service,
    delete_stateful_set, generate_random_secret, Context,
};

// A list of constants used in various K8s resources.
//
// When should you use a constant vs just hardcode the string literal directly?
//
// K8s uses lots of names to relate various resources (i.e. port names).
// We DO NOT want to use a constant for all of those use cases. However if a value
// spans multiple resources then we should create a constant for it.

/// Name of the config map maintained by the operator with information about each peer in the
/// network.
pub const PEERS_CONFIG_MAP_NAME: &str = "keramik-peers";

pub const CERAMIC_SERVICE_IPFS_PORT: i32 = 5001;
pub const CERAMIC_SERVICE_API_PORT: i32 = 7007;

pub const INIT_CONFIG_MAP_NAME: &str = "ceramic-init";
pub const ADMIN_SECRET_NAME: &str = "ceramic-admin";

pub const CAS_SERVICE_NAME: &str = "cas";
pub const CAS_IPFS_SERVICE_NAME: &str = "cas-ipfs";
pub const CAS_SERVICE_IPFS_PORT: i32 = 5001;
pub const CAS_POSTGRES_SERVICE_NAME: &str = "cas-postgres";
pub const CAS_POSTGRES_SECRET_NAME: &str = "postgres-auth";
pub const GANACHE_SERVICE_NAME: &str = "ganache";
pub const LOCALSTACK_SERVICE_NAME: &str = "localstack";

pub const CERAMIC_APP: &str = "ceramic";
pub const CAS_APP: &str = "cas";
pub const CAS_POSTGRES_APP: &str = "cas-postgres";
pub const CAS_IPFS_APP: &str = "cas-ipfs";
pub const GANACHE_APP: &str = "ganache";
pub const LOCALSTACK_APP: &str = "localstack";
pub const CERAMIC_LOCAL_NETWORK_TYPE: &str = "local";

pub const BOOTSTRAP_JOB_NAME: &str = "bootstrap";

/// Handle errors during reconciliation.
fn on_error(
    _network: Arc<Network>,
    _error: &Error,
    _context: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
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

/// Start a controller for the Network CRD.
pub async fn run() {
    let k_client = Client::try_default().await.unwrap();
    let context = Arc::new(
        Context::new(k_client.clone(), HttpRpcClient).expect("should be able to create context"),
    );

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
                    debug!(network.name, "reconcile success");
                }
                Err(err) => {
                    error!(?err, "reconcile error")
                }
            }
        })
        .await;
}

const MAX_CERAMICS: usize = 10;

/// Perform a reconcile pass for the Network CRD
async fn reconcile(
    network: Arc<Network>,
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
) -> Result<Action, Error> {
    let spec = network.spec();
    debug!(?spec, "reconcile");

    let mut status = if let Some(status) = &network.status {
        status.clone()
    } else {
        NetworkStatus::default()
    };
    if spec.ceramic.len() > MAX_CERAMICS {
        return Err(Error::App {
            source: anyhow!("too many ceramics configured, maximum {MAX_CERAMICS}"),
        });
    };

    // Check if the network should die, otherwise update expiration_time.
    let creation_timestamp = network.meta().creation_timestamp.as_ref();
    status.expiration_time = match (creation_timestamp, &spec.ttl_seconds) {
        (Some(created), Some(ttl_seconds)) => {
            let now = cx.clock.now();
            let expiration_time = created.0 + Duration::from_secs(*ttl_seconds);
            if now > expiration_time {
                info!("network TTL expired, deleting network");
                delete_network(cx.clone(), &network.name_unchecked()).await?;
                // Return early no need to update status
                return Ok(Action::requeue(Duration::from_secs(5 * 60)));
            };
            Some(Time(expiration_time))
        }
        (None, Some(_)) => {
            warn!("no creation time on network resource, cannot enforce TTL");
            None
        }
        _ => None,
    };

    let ns = apply_network_namespace(cx.clone(), network.clone()).await?;

    let net_config: NetworkConfig = spec.into();

    let datadog: DataDogConfig = (&spec.datadog).into();

    // Only create CAS resources if the Ceramic network was "local"
    let ceramic_configs: CeramicConfigs = spec.ceramic.clone().into();
    if net_config.network_type == CERAMIC_LOCAL_NETWORK_TYPE {
        apply_cas(cx.clone(), &ns, network.clone(), spec.cas.clone(), &datadog).await?;
    }

    if is_admin_secret_missing(cx.clone(), &ns).await? {
        create_admin_secret(
            cx.clone(),
            &ns,
            network.clone(),
            net_config.private_key_secret.as_ref(),
        )
        .await?;
    }

    let total_weight = ceramic_configs.0.iter().fold(0, |acc, c| acc + c.weight) as f64;
    let mut ceramics = Vec::with_capacity(ceramic_configs.0.len());
    for i in 0..MAX_CERAMICS {
        debug!(i, "ceramic check");
        let suffix = format!("{}", i);
        if let Some(config) = ceramic_configs.0.get(i) {
            let replicas = ((config.weight as f64 / total_weight) * spec.replicas as f64) as i32;
            let info = CeramicInfo::new(&suffix, replicas);

            ceramics.push(CeramicBundle {
                info,
                config,
                net_config: &net_config,
                datadog: &datadog,
            })
        } else {
            let info = CeramicInfo::new(&suffix, 0);
            debug!(?info, "deleting extra ceramic");
            delete_ceramic(cx.clone(), &ns, &info).await?;
        }
    }
    let computed_replicas = ceramics
        .iter()
        .fold(0, |acc, bundle| acc + bundle.info.replicas);
    if spec.replicas != computed_replicas {
        debug!(spec.replicas, computed_replicas, "replica counts");
        let diff = (spec.replicas - computed_replicas) as usize;
        let mut maxes: Vec<&mut CeramicBundle> = ceramics.iter_mut().collect();
        // Sort by maximum weight
        maxes.sort_by(|a, b| b.config.weight.cmp(&a.config.weight));
        // For the ceramics that have the maximum weight increase their replica counts by one.
        for max in maxes.into_iter().take(diff) {
            max.info.replicas += 1;
        }
    }

    for bundle in &ceramics {
        apply_ceramic(cx.clone(), &ns, network.clone(), bundle).await?;
    }

    let min_connected_peers = update_peer_status(
        cx.clone(),
        &ns,
        network.clone(),
        &ceramics,
        spec.replicas,
        &mut status,
    )
    .await?;
    debug!(min_connected_peers, "min_connected_peers");

    let bootstrap_config: BootstrapConfig = spec.bootstrap.clone().into();
    if bootstrap_config.enabled {
        // Check if we should rerun the bootstrap job.
        if let Some(min_connected_peers) = min_connected_peers {
            if status.peers.len() >= 2 && min_connected_peers == 0 {
                // We have ready peers that are not connected to any other peers.
                // Delete bootstrap job to rerun the job.
                reset_bootstrap_job(cx.clone(), &ns).await?;
            }
        }

        // Always apply the bootstrap job if we have at least 2 peers,
        // This way if the job is deleted externally for any reason it will rerun.
        if status.peers.len() >= 2 {
            apply_bootstrap_job(cx.clone(), &ns, network.clone(), bootstrap_config).await?;
        }
    }

    // Update network status
    let networks: Api<Network> = Api::all(cx.k_client.clone());
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
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    network: Arc<Network>,
) -> Result<String, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let namespaces: Api<Namespace> = Api::all(cx.k_client.clone());

    let ns = "keramik-".to_owned() + &network.name_any();
    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let namespace_data: Namespace = Namespace {
        metadata: ObjectMeta {
            name: Some(ns.clone()),
            owner_references: oref,
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

async fn delete_network(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    name: &str,
) -> Result<(), kube::error::Error> {
    let networks: Api<Network> = Api::all(cx.k_client.clone());

    networks.delete(name, &DeleteParams::default()).await?;
    Ok(())
}

async fn apply_cas(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    cas_spec: Option<CasSpec>,
    datadog: &DataDogConfig,
) -> Result<(), kube::error::Error> {
    if is_cas_postgres_secret_missing(cx.clone(), ns).await? {
        create_cas_postgres_secret(cx.clone(), ns, network.clone()).await?;
    }
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

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
        CAS_IPFS_SERVICE_NAME,
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
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        LOCALSTACK_SERVICE_NAME,
        cas::localstack_service_spec(),
    )
    .await?;

    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "cas",
        cas::cas_stateful_set_spec(ns, cas_spec.clone(), datadog),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "cas-ipfs",
        cas::cas_ipfs_stateful_set_spec(cas_spec.clone()),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "ganache",
        cas::ganache_stateful_set_spec(cas_spec.clone()),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "cas-postgres",
        cas::postgres_stateful_set_spec(cas_spec.clone()),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "localstack",
        cas::localstack_stateful_set_spec(cas_spec.clone()),
    )
    .await?;

    Ok(())
}

async fn is_cas_postgres_secret_missing(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, CAS_POSTGRES_SECRET_NAME).await
}
async fn create_cas_postgres_secret(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
) -> Result<(), kube::error::Error> {
    // Create postgres_secret
    let string_data = BTreeMap::from_iter(vec![
        ("username".to_owned(), "ceramic".to_owned()),
        (
            "password".to_owned(),
            generate_random_secret(cx.clone(), 20),
        ),
    ]);
    create_secret(cx, ns, network, CAS_POSTGRES_SECRET_NAME, string_data).await?;
    Ok(())
}

async fn is_admin_secret_missing(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, ADMIN_SECRET_NAME).await
}
async fn create_admin_secret(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    source_secret_name: Option<&String>,
) -> Result<(), kube::error::Error> {
    // If the name of a source secret was specified, look up that secret and use it to create the
    // new admin secret.
    let string_data = if let Some(source_secret_name) = source_secret_name {
        // Lookup the source secret in the "keramik" namespace
        let source_secret: Api<Secret> = Api::namespaced(cx.k_client.clone(), "keramik");
        from_utf8(
            &source_secret
                .get(source_secret_name)
                .await?
                .data
                .unwrap()
                .first_key_value()
                .unwrap()
                .1
                 .0,
        )
        .unwrap()
        .to_owned()
    } else {
        // If no source secret was specified create the new secret using a randomly generated value
        generate_random_secret(cx.clone(), 32)
    };
    create_secret(
        cx,
        ns,
        network,
        ADMIN_SECRET_NAME,
        BTreeMap::from_iter(vec![("private-key".to_owned(), string_data)]),
    )
    .await?;
    Ok(())
}

// Applies the ceramic related resources
async fn apply_ceramic<'a>(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    bundle: &CeramicBundle<'a>,
) -> Result<(), kube::error::Error> {
    let config_maps = ceramic::config_maps(&bundle.info, bundle.config);
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

    for (name, data) in config_maps {
        apply_config_map(cx.clone(), ns, orefs.clone(), &name, data).await?;
    }

    apply_ceramic_service(cx.clone(), ns, network.clone(), &bundle.info).await?;
    apply_ceramic_stateful_set(cx.clone(), ns, network.clone(), bundle).await?;

    Ok(())
}
// Deletes the configured ceramic
async fn delete_ceramic(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    info: &CeramicInfo,
) -> Result<(), kube::error::Error> {
    delete_stateful_set(cx.clone(), ns, &info.stateful_set).await?;
    delete_service(cx, ns, &info.service).await?;
    Ok(())
}

async fn apply_ceramic_service(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    info: &CeramicInfo,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

    apply_service(cx, ns, orefs, &info.service, ceramic::service_spec()).await
}

async fn apply_ceramic_stateful_set<'a>(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    bundle: &CeramicBundle<'a>,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let statefulset_name = bundle.info.stateful_set.to_owned();
    let spec = ceramic::stateful_set_spec(ns, bundle);
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();
    apply_stateful_set(cx, ns, orefs, &statefulset_name, spec).await
}

async fn apply_bootstrap_job(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    config: BootstrapConfig,
) -> Result<(), Error> {
    debug!("applying bootstrap job");
    let spec = bootstrap::bootstrap_job_spec(config);
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();
    apply_job(cx.clone(), ns, orefs, BOOTSTRAP_JOB_NAME, spec).await?;
    Ok(())
}

// Update status with current information about peers.
// Reports the minimum number of connected peers for any given peer.
// If not peers are ready None is returned.
async fn update_peer_status(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    ceramics: &[CeramicBundle<'_>],
    desired_replicas: i32,
    status: &mut NetworkStatus,
) -> Result<Option<i32>, Error> {
    status.replicas = desired_replicas;
    // Forget all previous status
    status.peers.clear();

    let pods: Api<Pod> = Api::namespaced(cx.k_client.clone(), ns);

    // Check status of all ceramic peers first
    for ceramic in ceramics {
        for i in 0..ceramic.info.replicas {
            let pod_name = ceramic.info.pod_name(i);
            let pod = pods.get_status(&pod_name).await?;
            if !is_pod_ready(&pod) {
                debug!(pod_name, "peer is not ready skipping");
                continue;
            }
            let ipfs_rpc_addr = ceramic.info.ipfs_rpc_addr(ns, i);
            let info = match cx.rpc_client.peer_info(&ipfs_rpc_addr).await {
                Ok(res) => res,
                Err(err) => {
                    debug!(%err, ipfs_rpc_addr, "failed to get peer info for ceramic peer");
                    continue;
                }
            };
            let ceramic_addr = ceramic.info.ceramic_addr(ns, i);
            status.peers.push(Peer::Ceramic(CeramicPeerInfo {
                ceramic_addr,
                peer_id: info.peer_id,
                ipfs_rpc_addr: info.ipfs_rpc_addr,
                p2p_addrs: info.p2p_addrs,
            }));
        }
    }
    // Update ready_replicas count
    status.ready_replicas = status.peers.len() as i32;

    // CAS IPFS peer
    let ipfs_rpc_addr = format!("http://{CAS_IPFS_SERVICE_NAME}-0.{CAS_IPFS_SERVICE_NAME}.{ns}.svc.cluster.local:{CAS_SERVICE_IPFS_PORT}");
    match cx.rpc_client.peer_info(&ipfs_rpc_addr).await {
        Ok(info) => {
            status.peers.push(Peer::Ipfs(info));
        }
        Err(err) => {
            trace!(%err, "failed to get peer info for cas-ipfs");
        }
    };

    // Determine the status of each peer
    let mut min_connected_peers = None;
    for peer in &status.peers {
        let peer_status = match cx.rpc_client.peer_status(peer.ipfs_rpc_addr()).await {
            Ok(res) => res,
            Err(err) => {
                warn!(%err, peer = peer.id(), "failed to get peer status for peer");
                continue;
            }
        };
        debug!(peer = peer.id(), ?peer_status, "peer status");
        min_connected_peers = Some(min(
            min_connected_peers.unwrap_or(peer_status.connected_peers),
            peer_status.connected_peers,
        ));
    }

    // Save the config map with the peer information
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

    apply_config_map(
        cx,
        ns,
        orefs,
        PEERS_CONFIG_MAP_NAME,
        peers::peer_config_map_data(&status.peers),
    )
    .await?;
    Ok(min_connected_peers)
}

fn is_pod_ready(pod: &Pod) -> bool {
    if let Some(status) = &pod.status {
        if let Some(conditions) = &status.conditions {
            for condition in conditions {
                if condition.type_ == "Ready" && condition.status == "True" {
                    return true;
                }
            }
        }
    }
    false
}

async fn is_secret_missing(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    name: &str,
) -> Result<bool, kube::error::Error> {
    let secrets: Api<Secret> = Api::namespaced(cx.k_client.clone(), ns);
    Ok(secrets.get_opt(name).await?.is_none())
}

async fn create_secret(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
    string_data: BTreeMap<String, String>,
) -> Result<(), kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let secrets: Api<Secret> = Api::namespaced(cx.k_client.clone(), ns);

    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let secret = Secret {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: oref,
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

// Deletes the bootstrap job if there is not an active job already running.
// Does nothing when the job does not exist.
async fn reset_bootstrap_job(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
) -> Result<(), kube::error::Error> {
    // Check if there is an active job already running.
    let jobs: Api<Job> = Api::namespaced(cx.k_client.clone(), ns);
    if let Some(job) = jobs.get_opt(BOOTSTRAP_JOB_NAME).await? {
        let running = job
            .status
            .map(|status| status.active.unwrap_or_default() > 0)
            .unwrap_or_default();
        if !running {
            jobs.delete(
                BOOTSTRAP_JOB_NAME,
                &DeleteParams {
                    // Delete resources in the foreground, otherwise job pods can get orphaned
                    // if we rapidly delete and apply the job.
                    propagation_policy: Some(kube::api::PropagationPolicy::Foreground),
                    ..Default::default()
                },
            )
            .await?;
        }
    } // no job exists, nothing to do
    Ok(())
}

// Stub tests relying on stub.rs and its apiserver stubs
#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, time::Duration};
    use std::{collections::HashMap, sync::Arc};

    use super::{reconcile, Network};

    use crate::{
        labels::managed_labels,
        network::{
            ipfs_rpc::{tests::MockIpfsRpcClientTest, PeerStatus},
            stub::{CeramicStub, Stub},
            BootstrapSpec, CasSpec, CeramicSpec, DataDogSpec, GoIpfsSpec, IpfsSpec, NetworkSpec,
            NetworkStatus, ResourceLimitsSpec, RustIpfsSpec,
        },
        utils::{
            test::{timeout_after_1s, ApiServerVerifier, WithStatus},
            Clock, Context,
        },
    };

    use expect_test::{expect, expect_file};
    use k8s_openapi::{
        api::{
            batch::v1::{Job, JobStatus},
            core::v1::{Pod, PodCondition, PodStatus, Secret},
        },
        apimachinery::pkg::{api::resource::Quantity, apis::meta::v1::Time},
        chrono::{DateTime, TimeZone, Utc},
        ByteString,
    };
    use keramik_common::peer_info::IpfsPeerInfo;
    use kube::Resource;
    use tracing::debug;
    use tracing_test::traced_test;

    #[derive(Clone, Copy)]
    struct StaticClock(DateTime<Utc>);
    impl Clock for StaticClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }
    // Construct default mock for IpfsRpc trait
    fn default_ipfs_rpc_mock() -> MockIpfsRpcClientTest {
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_cas_peer_info_not_ready(&mut mock_rpc_client);
        mock_rpc_client
    }
    fn ipfs_rpc_mock_n(n: usize) -> MockIpfsRpcClientTest {
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client
            .expect_peer_status()
            .times(n)
            .returning(|_| Ok(PeerStatus { connected_peers: 1 }));
        mock_rpc_client
            .expect_peer_info()
            .times(n)
            .returning(|addr| {
                Ok(IpfsPeerInfo {
                    peer_id: format!("peer_id_{addr}"),
                    ipfs_rpc_addr: addr.to_string(),
                    p2p_addrs: vec![],
                })
            });
        mock_rpc_client
    }
    // Mock for any peer that is connected
    fn mock_connected_peer_status(mock: &mut MockIpfsRpcClientTest) {
        mock.expect_peer_status()
            .once()
            .return_once(|_| Ok(PeerStatus { connected_peers: 1 }));
    }
    fn mock_not_connected_peer_status(mock: &mut MockIpfsRpcClientTest) {
        mock.expect_peer_status()
            .once()
            .return_once(|_| Ok(PeerStatus { connected_peers: 0 }));
    }

    // Mock for cas peer info call that is NOT ready
    fn mock_cas_peer_info_not_ready(mock: &mut MockIpfsRpcClientTest) {
        mock.expect_peer_info()
            .once()
            .return_once(|_| Err(anyhow::anyhow!("cas-ipfs not ready")));
    }
    // Mock for cas peer info call that is ready
    fn mock_cas_peer_info_ready(mock: &mut MockIpfsRpcClientTest) {
        mock.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "cas_peer_id".to_owned(),
                ipfs_rpc_addr: "http://cas-ipfs:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id".to_owned()],
            })
        });
    }

    fn not_ready_pod_status() -> Option<Pod> {
        Some(Pod {
            status: None,
            ..Default::default()
        })
    }
    fn ready_pod_status() -> Option<Pod> {
        Some(Pod {
            status: Some(PodStatus {
                conditions: Some(vec![PodCondition {
                    status: "True".to_string(),
                    type_: "Ready".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    // This tests defines the default stubs,
    // meaning the default stubs are the request response pairs
    // that occur when reconiling a default spec and status.
    #[tokio::test]
    #[traced_test]
    async fn reconcile_from_empty() {
        let mock_rpc_client = default_ipfs_rpc_mock();
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let network = Network::test();
        let stub = Stub::default();
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_two_peers_simple() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                replicas: 2,
                ..Default::default()
            })
            .with_status(NetworkStatus {
                replicas: 2,
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        // Setup peer info
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_0".to_owned(),
                ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
            })
        });
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_1".to_owned(),
                ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
            })
        });

        mock_cas_peer_info_ready(&mut mock_rpc_client);
        // Report that at least one peer is not connected so we need to bootstrap
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_not_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ceramic\":{\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"ceramicAddr\":\"http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}},{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,40 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "ceramicAddr": "http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_get"],
            Some(Job::default()),
        ));
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_delete"],
            Some(Job::default()),
        ));
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_apply"],
            Some(Job::default()),
        ));

        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    #[traced_test]
    async fn reconcile_two_peers_not_ready() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                replicas: 2,
                ..Default::default()
            })
            .with_status(NetworkStatus {
                replicas: 2,
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        // Setup peer info
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        // We expect only cas will be checked since both pods report they are not ready
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            not_ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            not_ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,20 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            +        "replicas": 2,
                     "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_two_peers_active_bootstrap() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                replicas: 2,
                ..Default::default()
            })
            .with_status(NetworkStatus {
                replicas: 2,
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        // Setup peer info
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_0".to_owned(),
                ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
            })
        });
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_1".to_owned(),
                ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
            })
        });
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        // Report that at least one peer is not connected so we need to bootstrap
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_not_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ceramic\":{\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"ceramicAddr\":\"http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}},{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,40 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "ceramicAddr": "http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_get"],
            Some(Job {
                status: Some(JobStatus {
                    active: Some(1),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        ));
        // NOTE: The job doesn't exist so we do not expect to see a DELETE request
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_apply"],
            Some(Job::default()),
        ));
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_two_peers_bootstrap_disabled() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                replicas: 2,
                bootstrap: Some(BootstrapSpec {
                    enabled: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                replicas: 2,
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        // Setup peer info
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_0".to_owned(),
                ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
            })
        });
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_1".to_owned(),
                ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
            })
        });
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        // Report that peers are not connected so we should bootstrap if it were enabled
        mock_not_connected_peer_status(&mut mock_rpc_client);
        mock_not_connected_peer_status(&mut mock_rpc_client);
        mock_not_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ceramic\":{\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"ceramicAddr\":\"http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}},{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,40 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "ceramicAddr": "http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        // Bootstrap is disabled, there should be no requests for it.
        stub.bootstrap_job = vec![];

        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_two_peers_no_bootstrap() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                replicas: 2,
                ..Default::default()
            })
            .with_status(NetworkStatus {
                replicas: 2,
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        // Setup peer info
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_0".to_owned(),
                ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
            })
        });
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_1".to_owned(),
                ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
            })
        });
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        // Report that peers are connected so we do not need to bootstrap;
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ceramic\":{\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"ceramicAddr\":\"http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}},{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,40 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "ceramicAddr": "http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        // Bootstrap is applied if we have at least two peers.
        // However we do not expect to see any GET/DELETE for the bootstrap job as all peers report
        // they are connected to other peers.
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_apply"],
            Some(Job::default()),
        ));
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_two_peers_twice() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                replicas: 2,
                ..Default::default()
            })
            .with_status(NetworkStatus {
                replicas: 2,
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        // Setup peer info
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_0".to_owned(),
                ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
            })
        });
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_1".to_owned(),
                ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
            })
        });
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        // Report all peers are connected
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ceramic\":{\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"ceramicAddr\":\"http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}},{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,40 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "ceramicAddr": "http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        // Bootstrap is applied if we have at least two peers.
        // However we do not expect to see any GET/DELETE for the bootstrap job as all peers report
        // they are connected to other peers.
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_apply"],
            Some(Job::default()),
        ));

        let network = {
            let (testctx, api_handle) = Context::test(mock_rpc_client);
            let fakeserver = ApiServerVerifier::new(api_handle);
            let mocksrv = stub.run(fakeserver);
            reconcile(Arc::new(network), testctx.clone())
                .await
                .expect("reconciler first");
            timeout_after_1s(mocksrv).await
        };

        debug!(?network, "network after one reconcile");

        //Setup peer info and stub for second pass.

        // Setup peer info, this is the same as before,
        // reconcile will always check on all peers each loop.
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_0".to_owned(),
                ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
            })
        });
        mock_rpc_client.expect_peer_info().once().return_once(|_| {
            Ok(IpfsPeerInfo {
                peer_id: "peer_id_1".to_owned(),
                ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
            })
        });
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        // Report all peers are connected
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -17,7 +17,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-0"].into(),
            ready_pod_status(),
        ));
        stub.ceramic_pod_status.push((
            expect_file!["./testdata/ceramic_pod_status-0-1"].into(),
            ready_pod_status(),
        ));
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ceramic\":{\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"ceramicAddr\":\"http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}},{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,40 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": [],
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-0-0.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ceramic": {
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "ceramicAddr": "http://ceramic-0-1.ceramic-0.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        // Bootstrap is applied if we have at least two peers.
        // However we do not expect to see any GET/DELETE for the bootstrap job as all peers report
        // they are connected to other peers.
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_two_peers_apply"],
            Some(Job::default()),
        ));

        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler second");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_cas_ipfs_peer() {
        let mut mock_rpc_client = MockIpfsRpcClientTest::new();
        mock_cas_peer_info_ready(&mut mock_rpc_client);
        mock_connected_peer_status(&mut mock_rpc_client);

        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let network = Network::test();
        let mut stub = Stub::default();
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ipfs\":{\"peerId\":\"cas_peer_id\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -10,7 +10,17 @@
                     "replicas": 0,
                     "readyReplicas": 0,
                     "namespace": null,
            -        "peers": [],
            +        "peers": [
            +          {
            +            "ipfs": {
            +              "peerId": "cas_peer_id",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.3/tcp/4001/p2p/cas_peer_id"
            +              ]
            +            }
            +          }
            +        ],
                     "expirationTime": null
                   }
                 },
        "#]]);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn go_ipfs_defaults() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                ceramic: vec![CeramicSpec {
                    ipfs: Some(IpfsSpec::Go(GoIpfsSpec::default())),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.ceramics[0]
            .configmaps
            .push(expect_file!["./testdata/go_ipfs_configmap"].into());
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -137,50 +137,8 @@
                             ]
                           },
                           {
            -                "env": [
            -                  {
            -                    "name": "CERAMIC_ONE_BIND_ADDRESS",
            -                    "value": "0.0.0.0:5001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_KADEMLIA_PARALLELISM",
            -                    "value": "1"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_KADEMLIA_REPLICATION",
            -                    "value": "6"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_LOCAL_NETWORK_ID",
            -                    "value": "0"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS",
            -                    "value": "true"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS_BIND_ADDRESS",
            -                    "value": "0.0.0.0:9465"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_NETWORK",
            -                    "value": "local"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_STORE_DIR",
            -                    "value": "/data/ipfs"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_SWARM_ADDRESSES",
            -                    "value": "/ip4/0.0.0.0/tcp/4001"
            -                  },
            -                  {
            -                    "name": "RUST_LOG",
            -                    "value": "info,ceramic_one=debug,tracing_actix_web=debug,quinn_proto=error"
            -                  }
            -                ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ipfs/kubo:v0.19.1@sha256:c4527752a2130f55090be89ade8dde8f8a5328ec72570676b90f66e2cabf827d",
            +                "imagePullPolicy": "IfNotPresent",
                             "name": "ipfs",
                             "ports": [
                               {
            @@ -215,6 +173,11 @@
                               {
                                 "mountPath": "/data/ipfs",
                                 "name": "ipfs-data"
            +                  },
            +                  {
            +                    "mountPath": "/container-init.d/001-config.sh",
            +                    "name": "ipfs-container-init-0",
            +                    "subPath": "001-config.sh"
                               }
                             ]
                           }
            @@ -323,6 +286,13 @@
                             "persistentVolumeClaim": {
                               "claimName": "ipfs-data"
                             }
            +              },
            +              {
            +                "configMap": {
            +                  "defaultMode": 493,
            +                  "name": "ipfs-container-init-0"
            +                },
            +                "name": "ipfs-container-init-0"
                           }
                         ]
                       }
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn go_ipfs_image() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                ceramic: vec![CeramicSpec {
                    ipfs: Some(IpfsSpec::Go(GoIpfsSpec {
                        image: Some("ipfs/ipfs:go".to_owned()),
                        resource_limits: Some(ResourceLimitsSpec {
                            cpu: Some(Quantity("4".to_owned())),
                            memory: Some(Quantity("4Gi".to_owned())),
                            storage: Some(Quantity("4Gi".to_owned())),
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.ceramics[0]
            .configmaps
            .push(expect_file!["./testdata/go_ipfs_configmap"].into());
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -137,50 +137,8 @@
                             ]
                           },
                           {
            -                "env": [
            -                  {
            -                    "name": "CERAMIC_ONE_BIND_ADDRESS",
            -                    "value": "0.0.0.0:5001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_KADEMLIA_PARALLELISM",
            -                    "value": "1"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_KADEMLIA_REPLICATION",
            -                    "value": "6"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_LOCAL_NETWORK_ID",
            -                    "value": "0"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS",
            -                    "value": "true"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS_BIND_ADDRESS",
            -                    "value": "0.0.0.0:9465"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_NETWORK",
            -                    "value": "local"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_STORE_DIR",
            -                    "value": "/data/ipfs"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_SWARM_ADDRESSES",
            -                    "value": "/ip4/0.0.0.0/tcp/4001"
            -                  },
            -                  {
            -                    "name": "RUST_LOG",
            -                    "value": "info,ceramic_one=debug,tracing_actix_web=debug,quinn_proto=error"
            -                  }
            -                ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ipfs/ipfs:go",
            +                "imagePullPolicy": "IfNotPresent",
                             "name": "ipfs",
                             "ports": [
                               {
            @@ -201,14 +159,14 @@
                             ],
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               }
                             },
                             "volumeMounts": [
            @@ -215,6 +173,11 @@
                               {
                                 "mountPath": "/data/ipfs",
                                 "name": "ipfs-data"
            +                  },
            +                  {
            +                    "mountPath": "/container-init.d/001-config.sh",
            +                    "name": "ipfs-container-init-0",
            +                    "subPath": "001-config.sh"
                               }
                             ]
                           }
            @@ -323,6 +286,13 @@
                             "persistentVolumeClaim": {
                               "claimName": "ipfs-data"
                             }
            +              },
            +              {
            +                "configMap": {
            +                  "defaultMode": 493,
            +                  "name": "ipfs-container-init-0"
            +                },
            +                "name": "ipfs-container-init-0"
                           }
                         ]
                       }
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn go_ipfs_commands() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                ceramic: vec![CeramicSpec {
                    ipfs: Some(IpfsSpec::Go(GoIpfsSpec {
                        commands: Some(vec![
                            "ipfs config Pubsub.SeenMessagesTTL 10m".to_owned(),
                            "ipfs config --json Swarm.RelayClient.Enabled false".to_owned(),
                        ]),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.ceramics[0]
            .configmaps
            .push(expect_file!["./testdata/go_ipfs_configmap_commands"].into());
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -137,50 +137,8 @@
                             ]
                           },
                           {
            -                "env": [
            -                  {
            -                    "name": "CERAMIC_ONE_BIND_ADDRESS",
            -                    "value": "0.0.0.0:5001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_KADEMLIA_PARALLELISM",
            -                    "value": "1"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_KADEMLIA_REPLICATION",
            -                    "value": "6"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_LOCAL_NETWORK_ID",
            -                    "value": "0"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS",
            -                    "value": "true"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS_BIND_ADDRESS",
            -                    "value": "0.0.0.0:9465"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_NETWORK",
            -                    "value": "local"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_STORE_DIR",
            -                    "value": "/data/ipfs"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_SWARM_ADDRESSES",
            -                    "value": "/ip4/0.0.0.0/tcp/4001"
            -                  },
            -                  {
            -                    "name": "RUST_LOG",
            -                    "value": "info,ceramic_one=debug,tracing_actix_web=debug,quinn_proto=error"
            -                  }
            -                ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ipfs/kubo:v0.19.1@sha256:c4527752a2130f55090be89ade8dde8f8a5328ec72570676b90f66e2cabf827d",
            +                "imagePullPolicy": "IfNotPresent",
                             "name": "ipfs",
                             "ports": [
                               {
            @@ -215,6 +173,16 @@
                               {
                                 "mountPath": "/data/ipfs",
                                 "name": "ipfs-data"
            +                  },
            +                  {
            +                    "mountPath": "/container-init.d/001-config.sh",
            +                    "name": "ipfs-container-init-0",
            +                    "subPath": "001-config.sh"
            +                  },
            +                  {
            +                    "mountPath": "/container-init.d/002-config.sh",
            +                    "name": "ipfs-container-init-0",
            +                    "subPath": "002-config.sh"
                               }
                             ]
                           }
            @@ -323,6 +291,13 @@
                             "persistentVolumeClaim": {
                               "claimName": "ipfs-data"
                             }
            +              },
            +              {
            +                "configMap": {
            +                  "defaultMode": 493,
            +                  "name": "ipfs-container-init-0"
            +                },
            +                "name": "ipfs-container-init-0"
                           }
                         ]
                       }
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn rust_ipfs_image() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                ceramic: vec![CeramicSpec {
                    ipfs: Some(IpfsSpec::Rust(RustIpfsSpec {
                        image: Some("ipfs/ipfs:rust".to_owned()),
                        resource_limits: Some(ResourceLimitsSpec {
                            cpu: Some(Quantity("4".to_owned())),
                            memory: Some(Quantity("4Gi".to_owned())),
                            storage: Some(Quantity("4Gi".to_owned())),
                        }),
                        env: Some(HashMap::from_iter([
                            ("ENV_KEY_A".to_string(), "ENV_VALUE_A".to_string()),
                            ("ENV_KEY_B".to_string(), "ENV_VALUE_B".to_string()),
                            // Override one existing var
                            ("CERAMIC_ONE_METRICS".to_string(), "false".to_string()),
                        ])),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -156,7 +156,7 @@
                               },
                               {
                                 "name": "CERAMIC_ONE_METRICS",
            -                    "value": "true"
            +                    "value": "false"
                               },
                               {
                                 "name": "CERAMIC_ONE_METRICS_BIND_ADDRESS",
            @@ -175,11 +175,19 @@
                                 "value": "/ip4/0.0.0.0/tcp/4001"
                               },
                               {
            +                    "name": "ENV_KEY_A",
            +                    "value": "ENV_VALUE_A"
            +                  },
            +                  {
            +                    "name": "ENV_KEY_B",
            +                    "value": "ENV_VALUE_B"
            +                  },
            +                  {
                                 "name": "RUST_LOG",
                                 "value": "info,ceramic_one=debug,tracing_actix_web=debug,quinn_proto=error"
                               }
                             ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            +                "image": "ipfs/ipfs:rust",
                             "imagePullPolicy": "Always",
                             "name": "ipfs",
                             "ports": [
            @@ -201,14 +209,14 @@
                             ],
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               }
                             },
                             "volumeMounts": [
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn cas_image() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                cas: Some(CasSpec {
                    image: Some("cas/cas:dev".to_owned()),
                    image_pull_policy: Some("Never".to_owned()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.cas_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -134,8 +134,8 @@
                                 "value": "9464"
                               }
                             ],
            -                "image": "ceramicnetwork/ceramic-anchor-service:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "cas/cas:dev",
            +                "imagePullPolicy": "Never",
                             "name": "cas-api",
                             "ports": [
                               {
            @@ -272,8 +272,8 @@
                                 "value": "false"
                               }
                             ],
            -                "image": "ceramicnetwork/ceramic-anchor-service:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "cas/cas:dev",
            +                "imagePullPolicy": "Never",
                             "name": "cas-worker",
                             "resources": {
                               "limits": {
            @@ -442,8 +442,8 @@
                                 "value": "dev"
                               }
                             ],
            -                "image": "ceramicnetwork/ceramic-anchor-service:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "cas/cas:dev",
            +                "imagePullPolicy": "Never",
                             "name": "cas-migrations"
                           },
                           {
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn cas_resource_limits() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                cas: Some(CasSpec {
                    cas_resource_limits: Some(ResourceLimitsSpec {
                        cpu: Some(Quantity("1".to_owned())),
                        memory: Some(Quantity("1Gi".to_owned())),
                        storage: Some(Quantity("1Gi".to_owned())),
                    }),
                    ipfs_resource_limits: Some(ResourceLimitsSpec {
                        cpu: Some(Quantity("2".to_owned())),
                        memory: Some(Quantity("2Gi".to_owned())),
                        storage: Some(Quantity("2Gi".to_owned())),
                    }),
                    ganache_resource_limits: Some(ResourceLimitsSpec {
                        cpu: Some(Quantity("3".to_owned())),
                        memory: Some(Quantity("3Gi".to_owned())),
                        storage: Some(Quantity("3Gi".to_owned())),
                    }),
                    postgres_resource_limits: Some(ResourceLimitsSpec {
                        cpu: Some(Quantity("4".to_owned())),
                        memory: Some(Quantity("4Gi".to_owned())),
                        storage: Some(Quantity("4Gi".to_owned())),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.cas_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -144,12 +144,12 @@
                             ],
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            +                    "cpu": "1",
                                 "ephemeral-storage": "1Gi",
                                 "memory": "1Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            +                    "cpu": "1",
                                 "ephemeral-storage": "1Gi",
                                 "memory": "1Gi"
                               }
            @@ -277,12 +277,12 @@
                             "name": "cas-worker",
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            +                    "cpu": "1",
                                 "ephemeral-storage": "1Gi",
                                 "memory": "1Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            +                    "cpu": "1",
                                 "ephemeral-storage": "1Gi",
                                 "memory": "1Gi"
                               }
            @@ -365,12 +365,12 @@
                             "name": "cas-scheduler",
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            +                    "cpu": "1",
                                 "ephemeral-storage": "1Gi",
                                 "memory": "1Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            +                    "cpu": "1",
                                 "ephemeral-storage": "1Gi",
                                 "memory": "1Gi"
                               }
        "#]]);
        stub.cas_ipfs_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -77,14 +77,14 @@
                             ],
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "2",
            +                    "ephemeral-storage": "2Gi",
            +                    "memory": "2Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "2",
            +                    "ephemeral-storage": "2Gi",
            +                    "memory": "2Gi"
                               }
                             },
                             "volumeMounts": [
        "#]]);
        stub.ganache_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -51,14 +51,14 @@
                             ],
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "1Gi"
            +                    "cpu": "3",
            +                    "ephemeral-storage": "3Gi",
            +                    "memory": "3Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "1Gi"
            +                    "cpu": "3",
            +                    "ephemeral-storage": "3Gi",
            +                    "memory": "3Gi"
                               }
                             },
                             "volumeMounts": [
        "#]]);
        stub.cas_postgres_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -67,14 +67,14 @@
                             ],
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "512Mi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               }
                             },
                             "volumeMounts": [
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_resource_limits() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                ceramic: vec![CeramicSpec {
                    resource_limits: Some(ResourceLimitsSpec {
                        cpu: Some(Quantity("4".to_owned())),
                        memory: Some(Quantity("4Gi".to_owned())),
                        storage: Some(Quantity("4Gi".to_owned())),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -115,14 +115,14 @@
                             },
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "1Gi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "1Gi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               }
                             },
                             "volumeMounts": [
            @@ -278,14 +278,14 @@
                             "name": "init-ceramic-config",
                             "resources": {
                               "limits": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "1Gi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               },
                               "requests": {
            -                    "cpu": "250m",
            -                    "ephemeral-storage": "1Gi",
            -                    "memory": "1Gi"
            +                    "cpu": "4",
            +                    "ephemeral-storage": "4Gi",
            +                    "memory": "4Gi"
                               }
                             },
                             "volumeMounts": [
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_admin_secret() {
        // Setup network spec with source secret name
        let network = Network::test().with_spec(NetworkSpec {
            private_key_secret: Some("private-key".to_owned()),
            ..Default::default()
        });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        // Tell the stub that the admin secret does not exist. This will make the controller attempt to create it using
        // the source secret.
        stub.ceramic_admin_secret_missing.1 = None;
        // Tell the stub to expect a call to lookup the source secret
        stub.ceramic_admin_secret_source = Some((
            expect_file!["./testdata/ceramic_source_admin_secret"].into(),
            Some(Secret {
                metadata: kube::core::ObjectMeta {
                    name: Some("private-key".to_owned()),
                    labels: managed_labels(),
                    ..kube::core::ObjectMeta::default()
                },
                data: Some(BTreeMap::from_iter(vec![(
                    "private-key".to_owned(),
                    ByteString(
                        "0e3b57bb4d269b6707019f75fe82fe06b1180dd762f183e96cab634e38d6e57b"
                            .as_bytes()
                            .to_vec(),
                    ),
                )])),
                ..Default::default()
            }),
            false,
        ));
        // Tell the stub to expect a call to create the admin secret
        stub.ceramic_admin_secret = Some((
            expect_file!["./testdata/ceramic_admin_secret"].into(),
            Some(Secret {
                metadata: kube::core::ObjectMeta {
                    name: Some("ceramic-admin".to_owned()),
                    labels: managed_labels(),
                    ..kube::core::ObjectMeta::default()
                },
                ..Default::default()
            }),
        ));
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_missing_admin_secret() {
        // Setup network spec with source secret name
        let network = Network::test().with_spec(NetworkSpec {
            private_key_secret: Some("private-key".to_owned()),
            ..Default::default()
        });
        let mock_rpc_client = MockIpfsRpcClientTest::new();
        let mut stub = Stub::default().with_network(network.clone());
        // Tell the stub that the admin secret does not exist. This will make the controller attempt to create it using
        // the source secret.
        stub.ceramic_admin_secret_missing.1 = None;
        // Tell the stub to expect a call to lookup the source secret. This will result in an error because the secret
        // is expected to be there if the name of the source secret was configured.
        stub.ceramic_admin_secret_source = Some((
            expect_file!["./testdata/ceramic_source_admin_secret"].into(),
            None,
            true, // skip the remainder of processing since an error will be returned
        ));
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        assert!(reconcile(Arc::new(network), testctx).await.is_err());
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_default_admin_secret() {
        // Setup default network spec
        let network = Network::test();
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        // Tell the stub that the secret does not exist. This will make the controller attempt to create it.
        stub.ceramic_admin_secret_missing.1 = None;
        // Tell the stub to expect a call to create the secret
        stub.ceramic_admin_secret_source = Some((
            expect_file!["./testdata/ceramic_default_admin_secret"].into(),
            Some(Secret {
                metadata: kube::core::ObjectMeta {
                    name: Some("ceramic-admin".to_owned()),
                    labels: managed_labels(),
                    ..kube::core::ObjectMeta::default()
                },
                ..Default::default()
            }),
            false,
        ));
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_external_cas() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                network_type: Some("dev-unstable".to_owned()),
                cas_api_url: Some("https://some-external-cas.com:8080".to_owned()),
                // Explicitly clear the PubSub topic and Ethereum RPC endpoint from the Ceramic configuration
                pubsub_topic: Some("".to_owned()),
                eth_rpc_url: Some("".to_owned()),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                ..Default::default()
            });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        // Tell the stub to skip all CAS-related configuration
        stub.postgres_auth_secret.2 = false;
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "status": {
                     "replicas": 0,
                     "readyReplicas": 0,
            -        "namespace": null,
            +        "namespace": "keramik-test",
                     "peers": [],
                     "expirationTime": null
                   }
        "#]]);
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -46,19 +46,19 @@
                             "env": [
                               {
                                 "name": "CERAMIC_NETWORK",
            -                    "value": "local"
            +                    "value": "dev-unstable"
                               },
                               {
                                 "name": "CERAMIC_NETWORK_TOPIC",
            -                    "value": "/ceramic/local-keramik"
            +                    "value": ""
                               },
                               {
                                 "name": "ETH_RPC_URL",
            -                    "value": "http://ganache:8545"
            +                    "value": ""
                               },
                               {
                                 "name": "CAS_API_URL",
            -                    "value": "http://cas:8081"
            +                    "value": "https://some-external-cas.com:8080"
                               },
                               {
                                 "name": "CERAMIC_SQLITE_PATH",
            @@ -238,19 +238,19 @@
                               },
                               {
                                 "name": "CERAMIC_NETWORK",
            -                    "value": "local"
            +                    "value": "dev-unstable"
                               },
                               {
                                 "name": "CERAMIC_NETWORK_TOPIC",
            -                    "value": "/ceramic/local-keramik"
            +                    "value": ""
                               },
                               {
                                 "name": "ETH_RPC_URL",
            -                    "value": "http://ganache:8545"
            +                    "value": ""
                               },
                               {
                                 "name": "CAS_API_URL",
            -                    "value": "http://cas:8081"
            +                    "value": "https://some-external-cas.com:8080"
                               },
                               {
                                 "name": "CERAMIC_SQLITE_PATH",
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_image() {
        // Setup network spec and status
        let network = Network::test().with_spec(NetworkSpec {
            ceramic: vec![CeramicSpec {
                image: Some("ceramic:foo".to_owned()),
                image_pull_policy: Some("IfNotPresent".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -81,8 +81,8 @@
                                 "value": "2"
                               }
                             ],
            -                "image": "ceramicnetwork/composedb:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ceramic:foo",
            +                "imagePullPolicy": "IfNotPresent",
                             "livenessProbe": {
                               "httpGet": {
                                 "path": "/api/v0/node/healthcheck",
            @@ -273,8 +273,8 @@
                                 "value": "2"
                               }
                             ],
            -                "image": "ceramicnetwork/composedb:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ceramic:foo",
            +                "imagePullPolicy": "IfNotPresent",
                             "name": "init-ceramic-config",
                             "resources": {
                               "limits": {
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn datadog() {
        // Setup network spec and status
        let network = Network::test().with_spec(NetworkSpec {
            datadog: Some(DataDogSpec {
                enabled: Some(true),
                version: Some("test".to_owned()),
                profiling_enabled: Some(true),
            }),
            ..Default::default()
        });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -27,11 +27,16 @@
                     "template": {
                       "metadata": {
                         "annotations": {
            +              "admission.datadoghq.com/js-lib.version": "latest",
                           "prometheus/path": "/metrics"
                         },
                         "labels": {
            +              "admission.datadoghq.com/enabled": "true",
                           "app": "ceramic",
            -              "managed-by": "keramik"
            +              "managed-by": "keramik",
            +              "tags.datadoghq.com/env": "keramik-test",
            +              "tags.datadoghq.com/service": "ceramic",
            +              "tags.datadoghq.com/version": "test"
                         }
                       },
                       "spec": {
            @@ -79,6 +84,22 @@
                               {
                                 "name": "CERAMIC_LOG_LEVEL",
                                 "value": "2"
            +                  },
            +                  {
            +                    "name": "DD_AGENT_HOST",
            +                    "valueFrom": {
            +                      "fieldRef": {
            +                        "fieldPath": "status.hostIP"
            +                      }
            +                    }
            +                  },
            +                  {
            +                    "name": "DD_RUNTIME_METRICS_ENABLED",
            +                    "value": "true"
            +                  },
            +                  {
            +                    "name": "DD_PROFILING_ENABLED",
            +                    "value": "true"
                               }
                             ],
                             "image": "ceramicnetwork/composedb:latest",
        "#]]);
        stub.cas_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -25,10 +25,16 @@
                     "serviceName": "cas",
                     "template": {
                       "metadata": {
            -            "annotations": {},
            +            "annotations": {
            +              "admission.datadoghq.com/js-lib.version": "latest"
            +            },
                         "labels": {
            +              "admission.datadoghq.com/enabled": "true",
                           "app": "cas",
            -              "managed-by": "keramik"
            +              "managed-by": "keramik",
            +              "tags.datadoghq.com/env": "keramik-test",
            +              "tags.datadoghq.com/service": "cas",
            +              "tags.datadoghq.com/version": "test"
                         }
                       },
                       "spec": {
            @@ -132,6 +138,22 @@
                               {
                                 "name": "METRICS_PROMETHEUS_PORT",
                                 "value": "9464"
            +                  },
            +                  {
            +                    "name": "DD_AGENT_HOST",
            +                    "valueFrom": {
            +                      "fieldRef": {
            +                        "fieldPath": "status.hostIP"
            +                      }
            +                    }
            +                  },
            +                  {
            +                    "name": "DD_RUNTIME_METRICS_ENABLED",
            +                    "value": "true"
            +                  },
            +                  {
            +                    "name": "DD_PROFILING_ENABLED",
            +                    "value": "true"
                               }
                             ],
                             "image": "ceramicnetwork/ceramic-anchor-service:latest",
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn multiple_default_ceramics() {
        // Setup network spec and status
        let network = Network::test().with_spec(NetworkSpec {
            ceramic: vec![CeramicSpec::default(), CeramicSpec::default()],
            ..Default::default()
        });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        // Remove first deletes
        stub.ceramic_deletes = stub.ceramic_deletes.into_iter().skip(2).collect();
        // Expect new ceramic
        stub.ceramics.push(CeramicStub {
            configmaps: vec![
                expect_file!["./testdata/default_stubs/ceramic_init_configmap"].into(),
            ],
            stateful_set: expect_file!["./testdata/ceramic_ss_1"].into(),
            service: expect_file!["./testdata/ceramic_svc_1"].into(),
        });
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn multiple_ceramics() {
        // Setup network spec and status
        let network = Network::test().with_spec(NetworkSpec {
            ceramic: vec![
                CeramicSpec::default(),
                CeramicSpec {
                    ipfs: Some(IpfsSpec::Go(GoIpfsSpec {
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        // Remove first deletes
        stub.ceramic_deletes = stub.ceramic_deletes.into_iter().skip(2).collect();
        // Expect new Go based ceramics.
        stub.ceramics.push(CeramicStub {
            configmaps: vec![
                expect_file!["./testdata/default_stubs/ceramic_init_configmap"].into(),
                expect_file!["./testdata/go_ipfs_configmap_1"].into(),
            ],
            stateful_set: expect_file!["./testdata/ceramic_go_ss_1"].into(),
            service: expect_file!["./testdata/ceramic_go_svc_1"].into(),
        });
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn multiple_weighted_ceramics() {
        // Setup network spec and status
        let weights = [10, 2, 1, 1, 1, 1, 1, 1, 1, 1];
        let replicas = 20;
        // To keep it simple we want weights to be the replica counts
        // Assert this is true.
        assert_eq!(weights.iter().sum::<i32>(), replicas);
        let network = Network::test().with_spec(NetworkSpec {
            replicas,
            ceramic: weights
                .iter()
                .map(|w| CeramicSpec {
                    weight: Some(*w),
                    ..Default::default()
                })
                .collect(),

            ..Default::default()
        });
        // + 1 for cas
        let mock_rpc_client = ipfs_rpc_mock_n(replicas as usize + 1);
        let mut stub = Stub::default().with_network(network.clone());
        // Remove first deletes
        stub.ceramic_deletes = Vec::new();
        // Expect new ceramics
        stub.ceramics = Vec::new();
        for i in 0..weights.len() {
            stub.ceramics.push(CeramicStub {
                configmaps: vec![
                    expect_file!["./testdata/default_stubs/ceramic_init_configmap"].into(),
                ],
                stateful_set: expect_file![format!("./testdata/ceramic_ss_weighted_{i}")].into(),
                service: expect_file![format!("./testdata/ceramic_svc_weighted_{i}")].into(),
            });
        }
        for (i, w) in weights.iter().enumerate() {
            for j in 0..*w {
                stub.ceramic_pod_status.push((
                    expect_file![format!(
                        "./testdata/multiple_weighted_ceramics/ceramic_pod_status-{i}-{j}"
                    )]
                    .into(),
                    ready_pod_status(),
                ));
            }
        }
        stub.keramik_peers_configmap = expect_file!["./testdata/ceramic_weighted_peers"].into();
        // Bootstrap is applied if we have at least two peers.
        // However we do not expect to see any GET/DELETE for the bootstrap job as all peers report
        // they are connected to other peers.
        stub.bootstrap_job.push((
            expect_file!["./testdata/bootstrap_job_many_peers_apply"],
            Some(Job::default()),
        ));
        stub.status = expect_file!["./testdata/ceramics_weighted_status"].into();

        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_expired() {
        // Expect no calls
        let mock_rpc_client = MockIpfsRpcClientTest::new();

        let clock = StaticClock(Utc.with_ymd_and_hms(2023, 10, 11, 9, 35, 0).unwrap());

        let (testctx, api_handle) = Context::test_with_clock(mock_rpc_client, clock);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mut network = Network::test().with_spec(NetworkSpec {
            // Set expiration as 5m from creation
            ttl_seconds: Some(300),
            ..Default::default()
        });
        // Set creation time as 10m ago
        network.meta_mut().creation_timestamp = Some(Time(clock.now() - Duration::from_secs(600)));

        // Expect network to be deleted
        let mut stub = Stub::default();
        stub.delete = Some(expect_file!["./testdata/delete_network"].into());

        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    #[traced_test]
    async fn reconcile_not_expired() {
        let mock_rpc_client = default_ipfs_rpc_mock();

        let clock = StaticClock(Utc.with_ymd_and_hms(2023, 10, 11, 9, 35, 0).unwrap());

        let (testctx, api_handle) = Context::test_with_clock(mock_rpc_client, clock);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mut network = Network::test().with_spec(NetworkSpec {
            // Set expiration as 10m into the future
            ttl_seconds: Some(600),
            ..Default::default()
        });
        // Set creation time as 5m ago
        network.meta_mut().creation_timestamp = Some(Time(clock.now() - Duration::from_secs(300)));

        // Expect network to report expiration time.
        let mut stub = Stub::default();
        stub.status = expect_file!["./testdata/not_expired_status"].into();

        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_environment() {
        // Setup network spec and status
        let mut env = HashMap::default();
        env.insert("SOME_ENV_VAR".to_string(), "SOME_ENV_VALUE".to_string());
        let network = Network::test().with_spec(NetworkSpec {
            ceramic: vec![CeramicSpec {
                env: Some(env),
                ..Default::default()
            }],
            ..Default::default()
        });
        let mock_rpc_client = default_ipfs_rpc_mock();
        let mut stub = Stub::default().with_network(network.clone());
        stub.ceramics[0].stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -79,6 +79,10 @@
                               {
                                 "name": "CERAMIC_LOG_LEVEL",
                                 "value": "2"
            +                  },
            +                  {
            +                    "name": "SOME_ENV_VAR",
            +                    "value": "SOME_ENV_VALUE"
                               }
                             ],
                             "image": "ceramicnetwork/composedb:latest",
            @@ -271,6 +275,10 @@
                               {
                                 "name": "CERAMIC_LOG_LEVEL",
                                 "value": "2"
            +                  },
            +                  {
            +                    "name": "SOME_ENV_VAR",
            +                    "value": "SOME_ENV_VALUE"
                               }
                             ],
                             "image": "ceramicnetwork/composedb:latest",
        "#]]);
        let (testctx, api_handle) = Context::test(mock_rpc_client);
        let fakeserver = ApiServerVerifier::new(api_handle);
        let mocksrv = stub.run(fakeserver);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
}
