use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetStatus},
        batch::v1::Job,
        core::v1::{ConfigMap, Namespace, Pod, Secret, Service, ServiceStatus},
    },
    apimachinery::pkg::util::intstr::IntOrString,
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
use tracing::{debug, error, trace};

use crate::network::{
    bootstrap,
    cas::{self, CasSpec},
    ceramic::{self, CeramicConfig},
    peers,
    utils::{ceramic_addr, ceramic_peer_ipfs_rpc_addr, HttpRpcClient, IpfsRpcClient},
    BootstrapSpec, CeramicSpec, Network, NetworkStatus,
};

use crate::utils::{
    apply_config_map, apply_job, apply_service, apply_stateful_set, generate_random_secret,
    managed_labels, Context, MANAGED_BY_LABEL_SELECTOR,
};

/// Handle errors during reconciliation.
fn on_error(
    _network: Arc<Network>,
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
                    debug!(network.name, "reconcile success");
                }
                Err(err) => {
                    error!(?err, "reconcile error")
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
pub const CERAMIC_SERVICE_IPFS_PORT: i32 = 5001;
pub const CERAMIC_SERVICE_API_PORT: i32 = 7007;

pub const INIT_CONFIG_MAP_NAME: &str = "ceramic-init";
pub const PEERS_CONFIG_MAP_NAME: &str = "keramik-peers";
pub const ADMIN_SECRET_NAME: &str = "ceramic-admin";

pub const CAS_SERVICE_NAME: &str = "cas";
pub const CAS_IFPS_SERVICE_NAME: &str = "cas-ipfs";
pub const CAS_SERVICE_IPFS_PORT: i32 = 5001;
pub const CAS_POSTGRES_SERVICE_NAME: &str = "cas-postgres";
pub const CAS_POSTGRES_SECRET_NAME: &str = "postgres-auth";
pub const GANACHE_SERVICE_NAME: &str = "ganache";

pub const CERAMIC_APP: &str = "ceramic";
pub const CAS_APP: &str = "cas";
pub const CAS_POSTGRES_APP: &str = "cas-postgres";
pub const CAS_IPFS_APP: &str = "cas-ipfs";
pub const GANACHE_APP: &str = "ganache";

pub const BOOTSTRAP_JOB_NAME: &str = "bootstrap";

/// Perform a reconcile pass for the Network CRD
async fn reconcile(
    network: Arc<Network>,
    cx: Arc<Context<impl IpfsRpcClient>>,
) -> Result<Action, Error> {
    let spec = network.spec();
    debug!(?spec, "reconcile");

    let mut status = if let Some(status) = &network.status {
        status.clone()
    } else {
        NetworkStatus::default()
    };

    let ns = apply_network_namespace(cx.clone(), network.clone()).await?;

    apply_cas(cx.clone(), &ns, network.clone(), spec.cas.clone()).await?;

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
            // Note since all *extra* peer use negative indexes they should always be retained.
            status.peers.retain(|peer| peer.index() < spec.replicas);
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
    cx: Arc<Context<impl IpfsRpcClient>>,
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
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
    cas_spec: Option<CasSpec>,
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
        cas::cas_stateful_set_spec(cas_spec.clone()),
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

    Ok(())
}

async fn is_cas_postgres_secret_missing(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, CAS_POSTGRES_SECRET_NAME).await
}
async fn create_cas_postgres_secret(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
) -> Result<(), kube::error::Error> {
    // Create postgres_secret
    let string_data = BTreeMap::from_iter(vec![
        ("username".to_owned(), "ceramic".to_owned()),
        (
            "password".to_owned(),
            hex::encode(generate_random_secret(20)),
        ),
    ]);
    create_secret(cx, ns, network, CAS_POSTGRES_SECRET_NAME, string_data).await?;
    Ok(())
}

async fn is_admin_secret_missing(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
) -> Result<bool, kube::error::Error> {
    is_secret_missing(cx, ns, ADMIN_SECRET_NAME).await
}
async fn create_admin_secret(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
    secret: String,
) -> Result<(), kube::error::Error> {
    let string_data = BTreeMap::from_iter(vec![("private-key".to_owned(), secret)]);
    create_secret(cx, ns, network, ADMIN_SECRET_NAME, string_data).await?;
    Ok(())
}
async fn apply_ceramic(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
    replicas: i32,
    spec: Option<CeramicSpec>,
) -> Result<(), kube::error::Error> {
    if is_admin_secret_missing(cx.clone(), &ns).await? {
        create_admin_secret(
            cx.clone(),
            &ns,
            network.clone(),
            spec.clone()
                .unwrap_or_default()
                .private_key
                .unwrap_or(generate_random_secret(32)),
        )
        .await?;
    }

    let config: CeramicConfig = spec.into();

    let config_maps = ceramic::config_maps(&config);
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

    for (name, data) in config_maps {
        apply_config_map(cx.clone(), ns, orefs.clone(), &name, data).await?;
    }

    apply_ceramic_service(cx.clone(), ns, network.clone()).await?;
    apply_ceramic_stateful_set(cx.clone(), ns, network.clone(), replicas, config).await?;

    Ok(())
}

async fn apply_ceramic_service(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

    apply_service(cx, ns, orefs, CERAMIC_SERVICE_NAME, ceramic::service_spec()).await
}

async fn apply_ceramic_stateful_set(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
    replicas: i32,
    config: CeramicConfig,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let spec = ceramic::stateful_set_spec(replicas, config);
    let orefs: Vec<_> = network
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();
    apply_stateful_set(cx, ns, orefs, CERAMIC_STATEFUL_SET_NAME, spec).await
}

async fn apply_bootstrap_job(
    cx: Arc<Context<impl IpfsRpcClient>>,
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
        let orefs: Vec<_> = network
            .controller_owner_ref(&())
            .map(|oref| vec![oref])
            .unwrap_or_default();
        apply_job(cx.clone(), ns, orefs, BOOTSTRAP_JOB_NAME, spec).await?;
    }
    Ok(())
}

async fn update_peer_info(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    network: Arc<Network>,
    status: &mut NetworkStatus,
) -> Result<(), Error> {
    // Check status of all ceramic peers first
    for index in 0..status.replicas {
        if status.peers.iter().any(|info| info.index() == index) {
            // Skip peers we already know
            continue;
        }
        let ipfs_rpc_addr = ceramic_peer_ipfs_rpc_addr(ns, index);
        let info = match cx.rpc_client.peer_info(index, ipfs_rpc_addr).await {
            Ok(res) => res,
            Err(err) => {
                trace!(%err, index, "failed to get peer info for ceramic peer");
                continue;
            }
        };
        let ceramic_addr = ceramic_addr(ns, index);
        status.peers.push(Peer::Ceramic(CeramicPeerInfo {
            ceramic_addr,
            index: info.index,
            peer_id: info.peer_id,
            ipfs_rpc_addr: info.ipfs_rpc_addr,
            p2p_addrs: info.p2p_addrs,
        }));
    }
    // ready_replicas count should not include other non ceramic peers
    status.ready_replicas = status.peers.len() as i32;

    // Add extra peers, using negative peer indexes
    {
        // CAS IPFS peer
        let ipfs_rpc_addr = format!("http://{CAS_IFPS_SERVICE_NAME}-0.{CAS_IFPS_SERVICE_NAME}.{ns}.svc.cluster.local:{CAS_SERVICE_IPFS_PORT}");
        match cx.rpc_client.peer_info(-1, ipfs_rpc_addr).await {
            Ok(info) => {
                status.peers.push(Peer::Ipfs(info));
            }
            Err(err) => {
                trace!(%err, "failed to get peer info for cas-ipfs");
            }
        };
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
    Ok(())
}

async fn is_secret_missing(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    name: &str,
) -> Result<bool, kube::error::Error> {
    let secrets: Api<Secret> = Api::namespaced(cx.k_client.clone(), ns);
    Ok(secrets.get_opt(name).await?.is_none())
}

async fn create_secret(
    cx: Arc<Context<impl IpfsRpcClient>>,
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

// Deletes a job. Does nothing if the job does not exist.
async fn delete_job(
    cx: Arc<Context<impl IpfsRpcClient>>,
    ns: &str,
    name: &str,
) -> Result<(), kube::error::Error> {
    let jobs: Api<Job> = Api::namespaced(cx.k_client.clone(), ns);
    if jobs.get_opt(name).await?.is_some() {
        jobs.delete(name, &DeleteParams::default()).await?;
    }
    Ok(())
}

// Stub tests relying on stub.rs and its apiserver stubs
#[cfg(test)]
mod test {
    use super::{reconcile, Network};

    use crate::{
        network::{
            cas::CasSpec,
            stub::{default_ipfs_rpc_mock, mock_cas_peer_info_not_ready, mock_cas_peer_info_ready},
            utils::ResourceLimitsSpec,
        },
        utils::Context,
    };

    use crate::network::{
        ceramic::{IpfsKind, IpfsSpec},
        stub::{timeout_after_1s, Stub},
        utils::RpcClientMock,
        CeramicSpec, NetworkSpec, NetworkStatus,
    };

    use expect_test::{expect, expect_file};
    use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
    use keramik_common::peer_info::IpfsPeerInfo;

    use crate::utils::generate_random_secret;
    use std::sync::Arc;
    use unimock::{matching, MockFn, Unimock};

    // This tests defines the default stubs,
    // meaning the default stubs are the request response pairs
    // that occur when reconiling a default spec and status.
    #[tokio::test]
    async fn reconcile_from_empty() {
        let mock_rpc_client = default_ipfs_rpc_mock();
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
            mock_cas_peer_info_not_ready(),
            RpcClientMock::peer_info
                .next_call(matching!(_))
                .returns(Ok(IpfsPeerInfo {
                    index: 0,
                    peer_id: "peer_id_0".to_owned(),
                    ipfs_rpc_addr: "http://peer0:5001".to_owned(),
                    p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
                })),
            RpcClientMock::peer_info
                .next_call(matching!(_))
                .returns(Ok(IpfsPeerInfo {
                    index: 1,
                    peer_id: "peer_id_1".to_owned(),
                    ipfs_rpc_addr: "http://peer1:5001".to_owned(),
                    p2p_addrs: vec!["/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1".to_owned()],
                })),
        ));
        let mut stub = Stub::default().with_network(network.clone());
        // Patch expected request values
        stub.ceramic_stateful_set.patch(expect![[r#"
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
        stub.keramik_peers_configmap.patch(expect![[r#"
            --- original
            +++ modified
            @@ -9,7 +9,7 @@
                   "apiVersion": "v1",
                   "kind": "ConfigMap",
                   "data": {
            -        "peers.json": "[]"
            +        "peers.json": "[{\"ceramic\":{\"index\":0,\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://peer0:5001\",\"ceramicAddr\":\"http://ceramic-1.ceramic.keramik-test.svc.cluster.local:7007\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}},{\"ipfs\":{\"index\":1,\"peerId\":\"peer_id_1\",\"ipfsRpcAddr\":\"http://peer1:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -7,10 +7,32 @@
                 },
                 body: {
                   "status": {
            -        "replicas": 0,
            -        "readyReplicas": 0,
            -        "namespace": null,
            -        "peers": []
            +        "replicas": 2,
            +        "readyReplicas": 1,
            +        "namespace": "keramik-test",
            +        "peers": [
            +          {
            +            "ceramic": {
            +              "index": 0,
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://peer0:5001",
            +              "ceramicAddr": "http://ceramic-1.ceramic.keramik-test.svc.cluster.local:7007",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          },
            +          {
            +            "ipfs": {
            +              "index": 1,
            +              "peerId": "peer_id_1",
            +              "ipfsRpcAddr": "http://peer1:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.2/tcp/4001/p2p/peer_id_1"
            +              ]
            +            }
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
    #[tokio::test]
    async fn reconcile_cas_ipfs_peer() {
        let mock_rpc_client = Unimock::new(mock_cas_peer_info_ready());
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
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
            +        "peers.json": "[{\"ipfs\":{\"index\":-1,\"peerId\":\"peer_id_0\",\"ipfsRpcAddr\":\"http://cas-ipfs:5001\",\"p2pAddrs\":[\"/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0\"]}}]"
                   },
                   "metadata": {
                     "labels": {
        "#]]);
        stub.bootstrap_job = Some(expect_file!["./testdata/bootstrap_job_cas_ipfs"]);
        stub.status.patch(expect![[r#"
            --- original
            +++ modified
            @@ -10,7 +10,18 @@
                     "replicas": 0,
                     "readyReplicas": 0,
                     "namespace": null,
            -        "peers": []
            +        "peers": [
            +          {
            +            "ipfs": {
            +              "index": -1,
            +              "peerId": "peer_id_0",
            +              "ipfsRpcAddr": "http://cas-ipfs:5001",
            +              "p2pAddrs": [
            +                "/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0"
            +              ]
            +            }
            +          }
            +        ]
                   }
                 },
             }
        "#]]);
        let mocksrv = fakeserver.run(stub);
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
                ceramic: Some(CeramicSpec {
                    ipfs: Some(IpfsSpec {
                        kind: IpfsKind::Go,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                peers: vec![],
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
                     "peers": []
                   }
                 },
        "#]]);
        stub.ceramic_configmaps
            .push(expect_file!["./testdata/go_ipfs_configmap"].into());
        stub.ceramic_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -128,39 +128,13 @@
                             ]
                           },
                           {
            -                "env": [
            -                  {
            -                    "name": "RUST_LOG",
            -                    "value": "info,ceramic_one=debug,tracing_actix_web=debug"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_BIND_ADDRESS",
            -                    "value": "0.0.0.0:5001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS",
            -                    "value": "true"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS_BIND_ADDRESS",
            -                    "value": "0.0.0.0:9090"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_SWARM_ADDRESSES",
            -                    "value": "/ip4/0.0.0.0/tcp/4001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_STORE_DIR",
            -                    "value": "/data/ipfs"
            -                  }
            -                ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ipfs/kubo:v0.19.1@sha256:c4527752a2130f55090be89ade8dde8f8a5328ec72570676b90f66e2cabf827d",
            +                "imagePullPolicy": "IfNotPresent",
                             "name": "ipfs",
                             "ports": [
                               {
                                 "containerPort": 4001,
            -                    "name": "swarm-tcp",
            +                    "name": "swarm",
                                 "protocol": "TCP"
                               },
                               {
            @@ -190,6 +164,11 @@
                               {
                                 "mountPath": "/data/ipfs",
                                 "name": "ipfs-data"
            +                  },
            +                  {
            +                    "mountPath": "/container-init.d/001-config.sh",
            +                    "name": "ipfs-container-init",
            +                    "subPath": "001-config.sh"
                               }
                             ]
                           }
            @@ -290,6 +269,13 @@
                             "persistentVolumeClaim": {
                               "claimName": "ipfs-data"
                             }
            +              },
            +              {
            +                "configMap": {
            +                  "defaultMode": 493,
            +                  "name": "ipfs-container-init"
            +                },
            +                "name": "ipfs-container-init"
                           }
                         ]
                       }
        "#]]);
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
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
                ceramic: Some(CeramicSpec {
                    ipfs: Some(IpfsSpec {
                        kind: IpfsKind::Go,
                        image: Some("ipfs/ipfs:go".to_owned()),
                        resource_limits: Some(ResourceLimitsSpec {
                            cpu: Some(Quantity("4".to_owned())),
                            memory: Some(Quantity("4Gi".to_owned())),
                            storage: Some(Quantity("4Gi".to_owned())),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                peers: vec![],
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
                     "peers": []
                   }
                 },
        "#]]);
        stub.ceramic_configmaps
            .push(expect_file!["./testdata/go_ipfs_configmap"].into());
        stub.ceramic_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -128,39 +128,13 @@
                             ]
                           },
                           {
            -                "env": [
            -                  {
            -                    "name": "RUST_LOG",
            -                    "value": "info,ceramic_one=debug,tracing_actix_web=debug"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_BIND_ADDRESS",
            -                    "value": "0.0.0.0:5001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS",
            -                    "value": "true"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_METRICS_BIND_ADDRESS",
            -                    "value": "0.0.0.0:9090"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_SWARM_ADDRESSES",
            -                    "value": "/ip4/0.0.0.0/tcp/4001"
            -                  },
            -                  {
            -                    "name": "CERAMIC_ONE_STORE_DIR",
            -                    "value": "/data/ipfs"
            -                  }
            -                ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "ipfs/ipfs:go",
            +                "imagePullPolicy": "IfNotPresent",
                             "name": "ipfs",
                             "ports": [
                               {
                                 "containerPort": 4001,
            -                    "name": "swarm-tcp",
            +                    "name": "swarm",
                                 "protocol": "TCP"
                               },
                               {
            @@ -176,14 +150,14 @@
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
            @@ -190,6 +164,11 @@
                               {
                                 "mountPath": "/data/ipfs",
                                 "name": "ipfs-data"
            +                  },
            +                  {
            +                    "mountPath": "/container-init.d/001-config.sh",
            +                    "name": "ipfs-container-init",
            +                    "subPath": "001-config.sh"
                               }
                             ]
                           }
            @@ -290,6 +269,13 @@
                             "persistentVolumeClaim": {
                               "claimName": "ipfs-data"
                             }
            +              },
            +              {
            +                "configMap": {
            +                  "defaultMode": 493,
            +                  "name": "ipfs-container-init"
            +                },
            +                "name": "ipfs-container-init"
                           }
                         ]
                       }
        "#]]);
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
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
                ceramic: Some(CeramicSpec {
                    ipfs: Some(IpfsSpec {
                        kind: IpfsKind::Rust,
                        image: Some("ipfs/ipfs:rust".to_owned()),
                        resource_limits: Some(ResourceLimitsSpec {
                            cpu: Some(Quantity("4".to_owned())),
                            memory: Some(Quantity("4Gi".to_owned())),
                            storage: Some(Quantity("4Gi".to_owned())),
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                peers: vec![],
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
                     "peers": []
                   }
                 },
        "#]]);
        stub.ceramic_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -154,7 +154,7 @@
                                 "value": "/data/ipfs"
                               }
                             ],
            -                "image": "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest",
            +                "image": "ipfs/ipfs:rust",
                             "imagePullPolicy": "Always",
                             "name": "ipfs",
                             "ports": [
            @@ -176,14 +176,14 @@
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
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
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
                peers: vec![],
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
                     "peers": []
                   }
                 },
        "#]]);
        stub.cas_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -120,8 +120,8 @@
                                 }
                               }
                             ],
            -                "image": "ceramicnetwork/ceramic-anchor-service:latest",
            -                "imagePullPolicy": "Always",
            +                "image": "cas/cas:dev",
            +                "imagePullPolicy": "Never",
                             "name": "cas",
                             "ports": [
                               {
        "#]]);
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
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
                peers: vec![],
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
                     "peers": []
                   }
                 },
        "#]]);
        stub.cas_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -130,12 +130,12 @@
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
        "#]]);
        stub.cas_ipfs_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -63,14 +63,14 @@
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
            @@ -57,14 +57,14 @@
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
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
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
                ceramic: Some(CeramicSpec {
                    resource_limits: Some(ResourceLimitsSpec {
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
                peers: vec![],
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
                     "peers": []
                   }
                 },
        "#]]);
        stub.ceramic_stateful_set.patch(expect![[r#"
            --- original
            +++ modified
            @@ -106,14 +106,14 @@
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
            @@ -245,14 +245,14 @@
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
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
    #[tokio::test]
    async fn ceramic_admin_secret() {
        // Setup network spec and status
        let network = Network::test()
            .with_spec(NetworkSpec {
                ceramic: Some(CeramicSpec {
                    resource_limits: Some(ResourceLimitsSpec {
                        cpu: Some(Quantity("250m".to_owned())),
                        memory: Some(Quantity("1Gi".to_owned())),
                        storage: Some(Quantity("1Gi".to_owned())),
                    }),
                    // private_key: Some(generate_random_secret(32)),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .with_status(NetworkStatus {
                ready_replicas: 0,
                namespace: Some("keramik-test".to_owned()),
                peers: vec![],
                ..Default::default()
            });
        // Setup peer info
        let mock_rpc_client = Unimock::new(());
        let mut stub = Stub::default().with_network(network.clone());
        let (testctx, fakeserver) = Context::test(mock_rpc_client);
        let mocksrv = fakeserver.run(stub);
        reconcile(Arc::new(network), testctx)
            .await
            .expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }
}
