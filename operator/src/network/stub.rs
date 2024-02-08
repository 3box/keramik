//! Helper methods only available for tests

use expect_patch::ExpectPatch;
use expect_test::{expect_file, ExpectFile};
use k8s_openapi::api::{
    apps::v1::StatefulSet,
    batch::v1::Job,
    core::v1::{Pod, Secret, Service},
};
use kube::{
    core::{ListMeta, ObjectList, TypeMeta},
    CustomResourceExt,
};

use crate::{
    labels::managed_labels,
    network::{Network, NetworkSpec, NetworkStatus, PodMonitor, PodMonitorSpec},
    utils::test::{ApiServerVerifier, WithStatus},
};

// Add tests specific implementation to the Network
impl Network {
    /// A normal test network
    pub fn test() -> Self {
        Network::new("test", NetworkSpec::default())
    }
    /// Modify a network to have an expected spec
    pub fn with_spec(self, spec: NetworkSpec) -> Self {
        Self { spec, ..self }
    }
}

impl WithStatus for Network {
    type Status = NetworkStatus;

    /// Modify a network to have an expected status
    fn with_status(self, status: NetworkStatus) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }
}

/// Stub of expected requests during reconciliation.
///
/// ```no_run
/// let mut stub = Stub::default();
/// // Patch the cas_service expected value.
/// // This patches the expected request the controller will make from its default.
/// // Default expecations are found in `./testdata/default_stubs`.
/// // Use `UPDATE_EXPECT=1 cargo test` to update all expect! including this patch.
/// stub.cas_service.patch(expect![[r#"..."#]]);
/// ```
#[derive(Debug)]
pub struct Stub {
    network: Network,
    pub delete: Option<ExpectPatch<ExpectFile>>,
    pub namespace: ExpectPatch<ExpectFile>,
    pub status: ExpectPatch<ExpectFile>,
    pub monitoring: Vec<ExpectFile>,
    pub pod_monitor: Vec<(
        (ExpectFile, bool),
        Option<(ExpectFile, bool)>,
        Option<ExpectFile>,
    )>,
    pub cas_postgres_auth_secret: (ExpectPatch<ExpectFile>, Secret, bool),
    pub ceramic_postgres_auth_secret: (ExpectPatch<ExpectFile>, Secret),
    pub ceramic_admin_secret_missing: (ExpectPatch<ExpectFile>, Option<Secret>),
    pub ceramic_admin_secret_source: Option<(ExpectPatch<ExpectFile>, Option<Secret>, bool)>,
    pub ceramic_admin_secret: Option<(ExpectPatch<ExpectFile>, Option<Secret>)>,
    pub ceramic_list_stateful_sets: (ExpectPatch<ExpectFile>, Option<ObjectList<StatefulSet>>),
    pub ceramic_list_services: (ExpectPatch<ExpectFile>, Option<ObjectList<Service>>),
    pub ceramic_deletes: Vec<ExpectPatch<ExpectFile>>,
    pub ceramic_pod_status: Vec<(ExpectPatch<ExpectFile>, Option<Pod>)>,
    pub keramik_peers_configmap: ExpectPatch<ExpectFile>,
    pub ceramics: Vec<CeramicStub>,
    pub cas_service: ExpectPatch<ExpectFile>,
    pub cas_ipfs_service: ExpectPatch<ExpectFile>,
    pub ganache_service: ExpectPatch<ExpectFile>,
    pub cas_postgres_service: ExpectPatch<ExpectFile>,
    pub localstack_service: ExpectPatch<ExpectFile>,
    pub cas_stateful_set: ExpectPatch<ExpectFile>,
    pub cas_ipfs_stateful_set: ExpectPatch<ExpectFile>,
    pub ganache_stateful_set: ExpectPatch<ExpectFile>,
    pub cas_postgres_stateful_set: ExpectPatch<ExpectFile>,
    pub localstack_stateful_set: ExpectPatch<ExpectFile>,
    pub bootstrap_job: Vec<(ExpectFile, Option<Job>)>,
}

#[derive(Debug)]
pub struct CeramicStub {
    pub configmaps: Vec<ExpectPatch<ExpectFile>>,
    pub stateful_set: ExpectPatch<ExpectFile>,
    pub service: ExpectPatch<ExpectFile>,
}

impl Default for Stub {
    fn default() -> Self {
        Self {
            delete: None,
            network: Network::test(),
            namespace: expect_file!["./testdata/default_stubs/namespace"].into(),
            status: expect_file!["./testdata/default_stubs/status"].into(),
            monitoring: vec![],
            pod_monitor: vec![],
            cas_postgres_auth_secret: (
                expect_file!["./testdata/default_stubs/postgres_auth_secret"].into(),
                k8s_openapi::api::core::v1::Secret {
                    metadata: kube::core::ObjectMeta {
                        name: Some("postgres-auth".to_owned()),
                        labels: managed_labels(),
                        ..kube::core::ObjectMeta::default()
                    },
                    ..Default::default()
                },
                true,
            ),
            ceramic_postgres_auth_secret: (
                expect_file!["./testdata/default_stubs/ceramic_postgres_auth_secret"].into(),
                k8s_openapi::api::core::v1::Secret {
                    metadata: kube::core::ObjectMeta {
                        name: Some("ceramic-postgres-auth".to_owned()),
                        labels: managed_labels(),
                        ..kube::core::ObjectMeta::default()
                    },
                    ..Default::default()
                },
            ),
            ceramic_admin_secret_missing: (
                expect_file!["./testdata/default_stubs/ceramic_admin_secret"].into(),
                Some(k8s_openapi::api::core::v1::Secret {
                    metadata: kube::core::ObjectMeta {
                        name: Some("ceramic-admin".to_owned()),
                        labels: managed_labels(),
                        ..kube::core::ObjectMeta::default()
                    },
                    ..Default::default()
                }),
            ),
            ceramic_admin_secret_source: None,
            ceramic_admin_secret: None,
            ceramic_list_stateful_sets: (
                expect_file!["./testdata/default_stubs/ceramic_stateful_set_list"].into(),
                Some(ObjectList {
                    items: vec![],
                    types: TypeMeta::default(),
                    metadata: ListMeta::default(),
                }),
            ),
            ceramic_list_services: (
                expect_file!["./testdata/default_stubs/ceramic_service_list"].into(),
                Some(ObjectList {
                    items: vec![],
                    types: TypeMeta::default(),
                    metadata: ListMeta::default(),
                }),
            ),
            ceramic_deletes: vec![],
            ceramic_pod_status: vec![],
            ceramics: vec![CeramicStub {
                configmaps: vec![
                    expect_file!["./testdata/default_stubs/ceramic_init_configmap"].into(),
                ],
                stateful_set: expect_file!["./testdata/default_stubs/ceramic_stateful_set"].into(),
                service: expect_file!["./testdata/default_stubs/ceramic_service"].into(),
            }],
            keramik_peers_configmap: expect_file![
                "./testdata/default_stubs/keramik_peers_configmap"
            ]
            .into(),
            cas_service: expect_file!["./testdata/default_stubs/cas_service"].into(),
            cas_ipfs_service: expect_file!["./testdata/default_stubs/cas_ipfs_service"].into(),
            ganache_service: expect_file!["./testdata/default_stubs/ganache_service"].into(),
            cas_postgres_service: expect_file!["./testdata/default_stubs/cas_postgres_service"]
                .into(),
            localstack_service: expect_file!["./testdata/default_stubs/localstack_service"].into(),
            cas_stateful_set: expect_file!["./testdata/default_stubs/cas_stateful_set"].into(),
            cas_ipfs_stateful_set: expect_file!["./testdata/default_stubs/cas_ipfs_stateful_set"]
                .into(),
            ganache_stateful_set: expect_file!["./testdata/default_stubs/ganache_stateful_set"]
                .into(),
            cas_postgres_stateful_set: expect_file![
                "./testdata/default_stubs/cas_postgres_stateful_set"
            ]
            .into(),
            localstack_stateful_set: expect_file![
                "./testdata/default_stubs/localstack_stateful_set"
            ]
            .into(),
            bootstrap_job: vec![],
        }
    }
}

impl Stub {
    pub fn with_network(self, network: Network) -> Self {
        Self { network, ..self }
    }

    /// Run a test with against the provided server.
    ///
    /// NB: If the controller is making more calls than we are handling in the stub,
    /// you then typically see a `KubeError(Service(Closed(())))` from the reconciler.
    ///
    /// You should await the `JoinHandle` (with a timeout) from this function to ensure that the
    /// stub runs to completion (i.e. all expected calls were responded to),
    /// using the timeout to catch missing api calls to Kubernetes.
    pub fn run(self, fakeserver: ApiServerVerifier) -> tokio::task::JoinHandle<Network> {
        tokio::spawn(self._run(fakeserver))
    }

    // Use explicit function since async closures are not yet supported
    async fn _run(self, mut fakeserver: ApiServerVerifier) -> Network {
        // We need to handle each expected call in sequence

        if let Some(delete) = self.delete {
            fakeserver
                .handle_request_response(delete, Some(&self.network))
                .await
                .expect("should be able to delete network");
            return self.network;
        }

        fakeserver
            .handle_apply(self.namespace)
            .await
            .expect("namespace should apply");
        for ((crd_get, crd_exists), monitor_get, monitor_post) in self.pod_monitor {
            let crd = if crd_exists {
                Some(PodMonitor::crd())
            } else {
                None
            };
            fakeserver
                .handle_request_response(crd_get, crd.as_ref())
                .await
                .expect("pod monitor crd should fetch");
            if let Some((monitor_get, monitor_exists)) = monitor_get {
                let monitor = if monitor_exists {
                    Some(PodMonitor::new("test", PodMonitorSpec::default()))
                } else {
                    None
                };
                fakeserver
                    .handle_request_response(monitor_get, monitor.as_ref())
                    .await
                    .expect("pod monitor should fetch");
            }
            if let Some(monitor_post) = monitor_post {
                fakeserver
                    .handle_request_response(
                        monitor_post,
                        Some(&PodMonitor::new("test", PodMonitorSpec::default())),
                    )
                    .await
                    .expect("pod monitor should create");
            }
        }
        for otel in self.monitoring {
            fakeserver
                .handle_apply(otel)
                .await
                .expect("opentelemetry should do work");
        }
        // Run/skip all CAS-related configuration
        if self.cas_postgres_auth_secret.2 {
            fakeserver
                .handle_request_response(
                    self.cas_postgres_auth_secret.0,
                    Some(&self.cas_postgres_auth_secret.1),
                )
                .await
                .expect("postgres-auth secret should exist");
            fakeserver
                .handle_apply(self.cas_service)
                .await
                .expect("cas service should apply");
            fakeserver
                .handle_apply(self.cas_ipfs_service)
                .await
                .expect("cas-ipfs service should apply");
            fakeserver
                .handle_apply(self.ganache_service)
                .await
                .expect("ganache service should apply");
            fakeserver
                .handle_apply(self.cas_postgres_service)
                .await
                .expect("cas-postgres service should apply");
            fakeserver
                .handle_apply(self.localstack_service)
                .await
                .expect("localstack service should apply");
            fakeserver
                .handle_apply(self.cas_stateful_set)
                .await
                .expect("cas stateful set should apply");
            fakeserver
                .handle_apply(self.cas_ipfs_stateful_set)
                .await
                .expect("cas-ipfs stateful set should apply");
            fakeserver
                .handle_apply(self.ganache_stateful_set)
                .await
                .expect("ganache stateful set should apply");
            fakeserver
                .handle_apply(self.cas_postgres_stateful_set)
                .await
                .expect("cas-postgres stateful set should apply");
            fakeserver
                .handle_apply(self.localstack_stateful_set)
                .await
                .expect("localstack stateful set should apply");
        }
        fakeserver
            .handle_request_response(
                self.ceramic_admin_secret_missing.0,
                self.ceramic_admin_secret_missing.1.as_ref(),
            )
            .await
            .expect("ceramic-admin secret should be looked up");
        if let Some(step) = self.ceramic_admin_secret_source {
            fakeserver
                .handle_request_response(step.0, step.1.as_ref())
                .await
                .expect("ceramic-admin source secret should be found");
            if step.2 {
                // skip the remainder of processing because this is an error case
                return self.network;
            }
        }
        if let Some(step) = self.ceramic_admin_secret {
            fakeserver
                .handle_request_response(step.0, step.1.as_ref())
                .await
                .expect("ceramic-admin secret should be created");
        }
        fakeserver
            .handle_request_response(
                self.ceramic_list_stateful_sets.0,
                self.ceramic_list_stateful_sets.1.as_ref(),
            )
            .await
            .expect("ceramic should list stateful sets");
        fakeserver
            .handle_request_response(
                self.ceramic_list_services.0,
                self.ceramic_list_services.1.as_ref(),
            )
            .await
            .expect("ceramic should list services");
        for ceramic_delete in self.ceramic_deletes {
            fakeserver
                .handle_request_response(ceramic_delete, None::<&StatefulSet>)
                .await
                .expect("ceramic should delete");
        }
        fakeserver
            .handle_request_response(
                self.ceramic_postgres_auth_secret.0,
                Some(&self.ceramic_postgres_auth_secret.1),
            )
            .await
            .expect("ceramic-postgres-auth secret should exist");
        for c in self.ceramics {
            for cm in c.configmaps {
                fakeserver
                    .handle_apply(cm)
                    .await
                    .expect("ceramic configmap should apply");
            }
            fakeserver
                .handle_apply(c.service)
                .await
                .expect("ceramic service should apply");
            fakeserver
                .handle_apply(c.stateful_set)
                .await
                .expect("ceramic stateful set should apply");
        }
        for ceramic_pod_status in self.ceramic_pod_status {
            fakeserver
                .handle_request_response(ceramic_pod_status.0, ceramic_pod_status.1.as_ref())
                .await
                .expect("ceramic pod status should exist");
        }
        fakeserver
            .handle_apply(self.keramik_peers_configmap)
            .await
            .expect("keramik-peers configmap should apply");
        for (req, resp) in self.bootstrap_job {
            fakeserver
                .handle_request_response(req, resp.as_ref())
                .await
                .expect("bootstrap job should apply");
        }
        fakeserver
            .handle_patch_status(self.status, self.network.clone())
            .await
            .expect("status should patch")
    }
}
