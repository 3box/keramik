//! Helper methods only available for tests

use expect_patch::ExpectPatch;
use expect_test::{expect_file, ExpectFile};
use k8s_openapi::api::{batch::v1::Job, core::v1::Secret};

use crate::{
    network::{Network, NetworkSpec, NetworkStatus},
    utils::{
        managed_labels,
        test::{ApiServerVerifier, WithStatus},
    },
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
    pub namespace: ExpectPatch<ExpectFile>,
    pub status: ExpectPatch<ExpectFile>,
    pub postgres_auth_secret: (ExpectPatch<ExpectFile>, Secret, bool),
    pub ceramic_admin_secret_missing: (ExpectPatch<ExpectFile>, Option<Secret>),
    pub ceramic_admin_secret_source: Option<(ExpectPatch<ExpectFile>, Option<Secret>, bool)>,
    pub ceramic_admin_secret: Option<(ExpectPatch<ExpectFile>, Option<Secret>)>,
    pub ceramic_stateful_set: ExpectPatch<ExpectFile>,
    pub keramik_peers_configmap: ExpectPatch<ExpectFile>,
    pub cas_service: ExpectPatch<ExpectFile>,
    pub cas_ipfs_service: ExpectPatch<ExpectFile>,
    pub ganache_service: ExpectPatch<ExpectFile>,
    pub cas_postgres_service: ExpectPatch<ExpectFile>,
    pub cas_stateful_set: ExpectPatch<ExpectFile>,
    pub cas_ipfs_stateful_set: ExpectPatch<ExpectFile>,
    pub ganache_stateful_set: ExpectPatch<ExpectFile>,
    pub cas_postgres_stateful_set: ExpectPatch<ExpectFile>,
    pub ceramic_configmaps: Vec<ExpectPatch<ExpectFile>>,
    pub ceramic_service: ExpectPatch<ExpectFile>,
    pub bootstrap_job: Vec<(ExpectFile, Option<Job>)>,
}

impl Default for Stub {
    fn default() -> Self {
        Self {
            network: Network::test(),
            namespace: expect_file!["./testdata/default_stubs/namespace"].into(),
            status: expect_file!["./testdata/default_stubs/status"].into(),
            postgres_auth_secret: (
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
            ceramic_stateful_set: expect_file!["./testdata/default_stubs/ceramic_stateful_set"]
                .into(),
            keramik_peers_configmap: expect_file![
                "./testdata/default_stubs/keramik_peers_configmap"
            ]
            .into(),
            cas_service: expect_file!["./testdata/default_stubs/cas_service"].into(),
            cas_ipfs_service: expect_file!["./testdata/default_stubs/cas_ipfs_service"].into(),
            ganache_service: expect_file!["./testdata/default_stubs/ganache_service"].into(),
            cas_postgres_service: expect_file!["./testdata/default_stubs/cas_postgres_service"]
                .into(),
            cas_stateful_set: expect_file!["./testdata/default_stubs/cas_stateful_set"].into(),
            cas_ipfs_stateful_set: expect_file!["./testdata/default_stubs/cas_ipfs_stateful_set"]
                .into(),
            ganache_stateful_set: expect_file!["./testdata/default_stubs/ganache_stateful_set"]
                .into(),
            cas_postgres_stateful_set: expect_file![
                "./testdata/default_stubs/cas_postgres_stateful_set"
            ]
            .into(),
            ceramic_configmaps: vec![expect_file![
                "./testdata/default_stubs/ceramic_init_configmap"
            ]
            .into()],
            ceramic_service: expect_file!["./testdata/default_stubs/ceramic_service"].into(),
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
        fakeserver
            .handle_apply(self.namespace)
            .await
            .expect("namespace should apply");
        // Run/skip all CAS-related configuration
        if self.postgres_auth_secret.2 {
            fakeserver
                .handle_request_response(
                    self.postgres_auth_secret.0,
                    Some(&self.postgres_auth_secret.1),
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
        for cm in self.ceramic_configmaps {
            fakeserver
                .handle_apply(cm)
                .await
                .expect("ceramic configmap should apply");
        }
        fakeserver
            .handle_apply(self.ceramic_service)
            .await
            .expect("ceramic service should apply");
        fakeserver
            .handle_apply(self.ceramic_stateful_set)
            .await
            .expect("ceramic stateful set should apply");
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
