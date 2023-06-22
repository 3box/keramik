//! Helper methods only available for tests

use anyhow::Result;
use expect_patch::{ExpectPatch, Expectation};
use expect_test::{expect_file, ExpectFile};
use hyper::Body;
use k8s_openapi::api::core::v1::Secret;
use keramik_common::peer_info::IpfsPeerInfo;
use kube::error::ErrorResponse;
use serde::Serialize;
use unimock::{matching, Clause, Unimock};

use crate::network::{utils::RpcClientMock, Network, NetworkSpec, NetworkStatus};

use crate::utils::{managed_labels, test::Request};

// We wrap tower_test::mock::Handle
type ApiServerHandle = tower_test::mock::Handle<http::Request<Body>, http::Response<Body>>;
pub struct ApiServerVerifier(ApiServerHandle);

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
    /// Modify a network to have an expected status
    pub fn with_status(self, status: NetworkStatus) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }
}

// Mock for cas peer info call that is NOT ready
pub fn mock_cas_peer_info_not_ready() -> impl Clause {
    RpcClientMock::peer_info
        .next_call(matching!(_))
        .returns(Err(anyhow::anyhow!("cas-ipfs not ready")))
}
// Mock for cas peer info call that is ready
pub fn mock_cas_peer_info_ready() -> impl Clause {
    RpcClientMock::peer_info
        .next_call(matching!(_))
        .returns(Ok(IpfsPeerInfo {
            index: -1,
            peer_id: "peer_id_0".to_owned(),
            ipfs_rpc_addr: "http://cas-ipfs:5001".to_owned(),
            p2p_addrs: vec!["/ip4/10.0.0.1/tcp/4001/p2p/peer_id_0".to_owned()],
        }))
}

// Construct default mock for IpfsRpc trait
pub fn default_ipfs_rpc_mock() -> Unimock {
    Unimock::new(mock_cas_peer_info_not_ready())
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
    pub bootstrap_job: Option<ExpectFile>,
}

impl Stub {
    pub fn with_network(self, network: Network) -> Self {
        Self { network, ..self }
    }
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
            bootstrap_job: None,
        }
    }
}

impl ApiServerVerifier {
    /// Create an ApiServerVerifier from a handle
    pub fn new(handle: ApiServerHandle) -> Self {
        Self(handle)
    }
    /// Run a test with the given stub.
    ///
    /// NB: If the controller is making more calls than we are handling in the stub,
    /// you then typically see a `KubeError(Service(Closed(())))` from the reconciler.
    ///
    /// You should await the `JoinHandle` (with a timeout) from this function to ensure that the
    /// stub runs to completion (i.e. all expected calls were responded to),
    /// using the timeout to catch missing api calls to Kubernetes.
    pub fn run(mut self, stub: Stub) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // We need to handle each expected call in sequence
            self.handle_apply(stub.namespace)
                .await
                .expect("namespace should apply");
            // Run/skip all CAS-related configuration
            if stub.postgres_auth_secret.2 {
                self.handle_request_response(
                    stub.postgres_auth_secret.0,
                    Some(&stub.postgres_auth_secret.1),
                )
                .await
                .expect("postgres-auth secret should exist");
                self.handle_apply(stub.cas_service)
                    .await
                    .expect("cas service should apply");
                self.handle_apply(stub.cas_ipfs_service)
                    .await
                    .expect("cas-ipfs service should apply");
                self.handle_apply(stub.ganache_service)
                    .await
                    .expect("ganache service should apply");
                self.handle_apply(stub.cas_postgres_service)
                    .await
                    .expect("cas-postgres service should apply");
                self.handle_apply(stub.cas_stateful_set)
                    .await
                    .expect("cas stateful set should apply");
                self.handle_apply(stub.cas_ipfs_stateful_set)
                    .await
                    .expect("cas-ipfs stateful set should apply");
                self.handle_apply(stub.ganache_stateful_set)
                    .await
                    .expect("ganache stateful set should apply");
                self.handle_apply(stub.cas_postgres_stateful_set)
                    .await
                    .expect("cas-postgres stateful set should apply");
            }
            self.handle_request_response(
                stub.ceramic_admin_secret_missing.0,
                stub.ceramic_admin_secret_missing.1.as_ref(),
            )
            .await
            .expect("ceramic-admin secret should be looked up");
            if let Some(step) = stub.ceramic_admin_secret_source {
                self.handle_request_response(step.0, step.1.as_ref())
                    .await
                    .expect("ceramic-admin source secret should be found");
                if step.2 {
                    // skip the remainder of processing because this is an error case
                    return;
                }
            }
            if let Some(step) = stub.ceramic_admin_secret {
                self.handle_request_response(step.0, step.1.as_ref())
                    .await
                    .expect("ceramic-admin secret should be created");
            }
            for cm in stub.ceramic_configmaps {
                self.handle_apply(cm)
                    .await
                    .expect("ceramic configmap should apply");
            }
            self.handle_apply(stub.ceramic_service)
                .await
                .expect("ceramic service should apply");
            self.handle_apply(stub.ceramic_stateful_set)
                .await
                .expect("ceramic stateful set should apply");
            self.handle_apply(stub.keramik_peers_configmap)
                .await
                .expect("keramik-peers configmap should apply");
            if let Some(bootstrap_job) = stub.bootstrap_job {
                self.handle_apply(bootstrap_job)
                    .await
                    .expect("bootstrap job should apply");
            }
            self.handle_patch_status(stub.status, stub.network.clone())
                .await
                .expect("status should patch");
        })
    }

    async fn handle_patch_status(
        &mut self,
        expected_request: impl Expectation,
        network: Network,
    ) -> Result<()> {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        let json: serde_json::Value =
            serde_json::from_str(&request.body.0).expect("status should be JSON");

        let status_json = json.get("status").expect("status object").clone();
        let status: NetworkStatus =
            serde_json::from_value(status_json).expect("JSON should be a valid status");

        let network = network.with_status(status);
        let response = serde_json::to_vec(&network).unwrap();
        send.send_response(
            http::Response::builder()
                .body(Body::from(response))
                .unwrap(),
        );
        Ok(())
    }

    async fn handle_apply(&mut self, expected_request: impl Expectation) -> Result<()> {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        send.send_response(
            http::Response::builder()
                .body(Body::from(request.body.0))
                .unwrap(),
        );
        Ok(())
    }
    async fn handle_request_response<T>(
        &mut self,
        expected_request: impl Expectation,
        response: Option<&T>,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        let response = if let Some(response) = response {
            http::Response::builder()
                .body(Body::from(serde_json::to_vec(response).unwrap()))
                .unwrap()
        } else {
            let error = ErrorResponse {
                status: "stub status".to_owned(),
                code: 0,
                message: "stub message".to_owned(),
                reason: "NotFound".to_owned(),
            };
            http::Response::builder()
                .status(404)
                .body(Body::from(serde_json::to_vec(&error).unwrap()))
                .unwrap()
        };
        send.send_response(response);
        Ok(())
    }
}
