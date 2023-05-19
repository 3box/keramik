use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetSpec, StatefulSetStatus},
        batch::v1::{Job, JobSpec, JobStatus},
        core::v1::{ConfigMap, Namespace, Pod, Secret, Service, ServiceSpec, ServiceStatus},
    },
    apimachinery::pkg::util::intstr::IntOrString,
};
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
    peers,
    utils::{HttpRpcClient, RpcClient},
    BootstrapSpec, CeramicSpec, Network, NetworkStatus,
};

//can add resource apis here
pub struct Context<R> {
    pub k_client: Client,
    pub rpc_client: R,
}

impl<R> Context<R> {
    pub fn new(k_client: Client, rpc_client: R) -> Self
    where
        R: RpcClient,
    {
        Context {
            k_client,
            rpc_client,
        }
    }
}

/// Handle errors during reconciliation.
fn on_error(
    _network: Arc<Network>,
    _error: &Error,
    _context: Arc<Context<impl RpcClient>>,
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
    let context = Arc::new(Context::new(k_client.clone(), HttpRpcClient));

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

// Create lables that can be used as a unique selector for a given app name.
pub fn selector_labels(app: &str) -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from_iter(vec![(
        "app".to_owned(),
        app.to_owned(),
    )]))
}

pub const MANAGED_BY_LABEL_SELECTOR: &str = "managed-by=keramik";

// Labels that indicate the resource is managed by the keramik operator.
pub fn managed_labels() -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from_iter(vec![(
        "managed-by".to_owned(),
        "keramik".to_owned(),
    )]))
}

/// Perform a reconile pass for the Network CRD
async fn reconcile(
    network: Arc<Network>,
    cx: Arc<Context<impl RpcClient>>,
) -> Result<Action, Error> {
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
    cx: Arc<Context<impl RpcClient>>,
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

async fn apply_cas(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
) -> Result<(), kube::error::Error> {
    if is_cas_postgres_secret_missing(cx.clone(), ns).await? {
        create_cas_postgres_secret(cx.clone(), ns, network.clone()).await?;
    }
    apply_service(
        cx.clone(),
        ns,
        network.clone(),
        CAS_SERVICE_NAME,
        cas::cas_service_spec(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        network.clone(),
        CAS_IFPS_SERVICE_NAME,
        cas::cas_ipfs_service_spec(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        network.clone(),
        GANACHE_SERVICE_NAME,
        cas::ganache_service_spec(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        network.clone(),
        CAS_POSTGRES_SERVICE_NAME,
        cas::postgres_service_spec(),
    )
    .await?;

    apply_stateful_set(
        cx.clone(),
        ns,
        network.clone(),
        "cas",
        cas::cas_stateful_set_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        network.clone(),
        "cas-ipfs",
        cas::cas_ipfs_stateful_set_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        network.clone(),
        "ganache",
        cas::ganache_stateful_set_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        network.clone(),
        "cas-postgres",
        cas::postgres_stateful_set_spec(),
    )
    .await?;

    Ok(())
}

async fn is_cas_postgres_secret_missing(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, CAS_POSTGRES_SECRET_NAME).await
}
async fn create_cas_postgres_secret(
    cx: Arc<Context<impl RpcClient>>,
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
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, ADMIN_SECRET_NAME).await
}
async fn create_admin_secret(
    cx: Arc<Context<impl RpcClient>>,
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
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    replicas: i32,
    spec: Option<CeramicSpec>,
) -> Result<(), kube::error::Error> {
    let config: CeramicConfig = spec.into();

    if config.init_config_map == INIT_CONFIG_MAP_NAME {
        //Only create the config map if its the one we own.
        apply_init_config(cx.clone(), ns, network.clone(), &config.init_config_map).await?;
    }

    apply_ceramic_service(cx.clone(), ns, network.clone()).await?;
    apply_ceramic_stateful_set(cx.clone(), ns, network.clone(), replicas, config).await?;

    Ok(())
}

async fn apply_init_config(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
) -> Result<(), kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let config_maps: Api<ConfigMap> = Api::namespaced(cx.k_client.clone(), ns);
    // Apply config map
    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let map_data = ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: oref,
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        data: Some(ceramic::init_config_map_data()),
        ..Default::default()
    };
    config_maps
        .patch(name, &serverside, &Patch::Apply(map_data))
        .await?;
    Ok(())
}

async fn apply_ceramic_service(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    apply_service(
        cx,
        ns,
        network,
        CERAMIC_SERVICE_NAME,
        ceramic::service_spec(),
    )
    .await
}

async fn apply_ceramic_stateful_set(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    replicas: i32,
    config: CeramicConfig,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let spec = ceramic::stateful_set_spec(replicas, config);
    apply_stateful_set(cx, ns, network, CERAMIC_STATEFUL_SET_NAME, spec).await
}

async fn apply_bootstrap_job(
    cx: Arc<Context<impl RpcClient>>,
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
    if ready > 0 && ready as f64 >= replicas as f64 * percent {
        debug!("creating bootstrap job");
        // Create bootstrap jobs
        let spec = bootstrap::bootstrap_job_spec(spec);
        apply_job(cx.clone(), ns, network.clone(), BOOTSTRAP_JOB_NAME, spec).await?;
    }
    Ok(())
}

async fn update_peer_info(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    status: &mut NetworkStatus,
) -> Result<(), Error> {
    println!("update_peer_info {:?}", status);
    for index in 0..status.replicas {
        if status.peers.iter().any(|info| info.index == index) {
            // Skip peers we already know
            continue;
        }
        let info = match cx.rpc_client.peer_info(ns, index).await {
            Ok(res) => res,
            Err(err) => {
                trace!(%err, index, "failed to get peer id and mulitaddrs for peer");
                continue;
            }
        };
        status.peers.push(info);
    }
    status.ready_replicas = status.peers.len() as i32;
    apply_config_map(
        cx,
        ns,
        network,
        PEERS_CONFIG_MAP_NAME,
        peers::peer_config_map_data(&status.peers),
    )
    .await?;
    Ok(())
}

async fn apply_service(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
    spec: ServiceSpec,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let services: Api<Service> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply service
    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let service: Service = Service {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: oref,
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        spec: Some(spec),
        ..Default::default()
    };
    let service = services
        .patch(name, &serverside, &Patch::Apply(service))
        .await?;
    Ok(service.status)
}

async fn apply_stateful_set(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
    spec: StatefulSetSpec,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply stateful_set
    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let stateful_set: StatefulSet = StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: oref,
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        spec: Some(spec),
        ..Default::default()
    };
    let stateful_set = stateful_sets
        .patch(name, &serverside, &Patch::Apply(stateful_set))
        .await?;
    Ok(stateful_set.status)
}

async fn is_secret_missing(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    name: &str,
) -> Result<bool, kube::error::Error> {
    let secrets: Api<Secret> = Api::namespaced(cx.k_client.clone(), ns);
    Ok(secrets.get_opt(name).await?.is_none())
}
async fn create_secret(
    cx: Arc<Context<impl RpcClient>>,
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

async fn apply_job(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
    spec: JobSpec,
) -> Result<Option<JobStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let jobs: Api<Job> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply stateful_set
    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let job: Job = Job {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: oref,
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        spec: Some(spec),
        ..Default::default()
    };
    let job = jobs.patch(name, &serverside, &Patch::Apply(job)).await?;
    Ok(job.status)
}

// Deletes a job. Does nothing if the job does not exist.
async fn delete_job(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    name: &str,
) -> Result<(), kube::error::Error> {
    let jobs: Api<Job> = Api::namespaced(cx.k_client.clone(), ns);
    if jobs.get_opt(name).await?.is_some() {
        jobs.delete(name, &DeleteParams::default()).await?;
    }
    Ok(())
}

async fn apply_config_map(
    cx: Arc<Context<impl RpcClient>>,
    ns: &str,
    network: Arc<Network>,
    name: &str,
    data: BTreeMap<String, String>,
) -> Result<(), kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let config_maps: Api<ConfigMap> = Api::namespaced(cx.k_client.clone(), ns);
    // Apply config map
    let oref: Option<Vec<_>> = network.controller_owner_ref(&()).map(|oref| vec![oref]);
    let map_data = ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: oref,
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        data: Some(data),
        ..Default::default()
    };
    config_maps
        .patch(name, &serverside, &Patch::Apply(map_data))
        .await?;
    Ok(())
}

// Stub tests relying on stub.rs and its apiserver stubs
#[cfg(test)]
mod test {
    use super::{reconcile, Context, Network};

    use crate::network::{
        stub::{timeout_after_1s, Stub},
        utils::RpcClientMock,
        NetworkSpec, NetworkStatus,
    };

    use expect_test::{expect, expect_file};
    use keramik_common::peer_info::PeerInfo;

    use std::sync::Arc;
    use unimock::{matching, MockFn, Unimock};

    // This tests defines the default stubs,
    // meaning the default stubs are the request response pairs
    // that occur when reconiling a default spec and status.
    #[tokio::test]
    async fn reconcile_from_empty() {
        let mock_rpc_client = Unimock::new(());
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let network = Network::test();
        let mocksrv = fakeserver.run(Stub::default());
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn reconcile_two_peers() {
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
                peers: vec![],
            });
        // Setup peer info
        let mock_rpc_client = Unimock::new((
            RpcClientMock::peer_info
                .next_call(matching!(_))
                .returns(Ok(PeerInfo {
                    index: 0,
                    peer_id: "peer_id_0".to_owned(),
                    rpc_addr: "http://peer0".to_owned(),
                    p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
                })),
            RpcClientMock::peer_info
                .next_call(matching!(_))
                .returns(Ok(PeerInfo {
                    index: 1,
                    peer_id: "peer_id_1".to_owned(),
                    rpc_addr: "http://peer1".to_owned(),
                    p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
                })),
        ));
        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramic_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -16,7 +16,7 @@
                   },
                   "spec": {
                     "podManagementPolicy": "Parallel",
            -        "replicas": 0,
            +        "replicas": 2,
                     "selector": {
                       "matchLabels": {
                         "app": "ceramic"
        "#]]);
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"index\":0,\"peerId\":\"peer_id_0\",\"rpcAddr\":\"http://peer0\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]},{\"index\":1,\"peerId\":\"peer_id_1\",\"rpcAddr\":\"http://peer1\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,27 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": []
            +        "replicas": 2,
            +        "readyReplicas": 2,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "index": 0,
            +            "peerId": "peer_id_0",
            +            "rpcAddr": "http://peer0",
            +            "p2pAddrs": [
            +              "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +            ]
            +          },
            +          {
            +            "index": 1,
            +            "peerId": "peer_id_1",
            +            "rpcAddr": "http://peer1",
            +            "p2pAddrs": [
            +              "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +            ]
            +          }
            +        ]
                   }
                 },
             }
        "#]]);
        stub.bootstrap_job = Some(expect_file!["./testdata/bootstrap_job_two_peers"]);
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
}
