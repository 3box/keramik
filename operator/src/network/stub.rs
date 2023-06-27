//! Helper methods only available for tests

use anyhow::{anyhow, Result};
use expect_test::{expect_file, Expect, ExpectFile};
use hyper::{body::to_bytes, Body};
use k8s_openapi::api::core::v1::Secret;
use keramik_common::peer_info::IpfsPeerInfo;
use kube::error::ErrorResponse;
use kube::Client;
use rand::rngs::mock::StepRng;
use rand::RngCore;
use reqwest::header::HeaderMap;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use unimock::{matching, Clause, MockFn, Unimock};

use crate::network::{
    utils::{IpfsRpcClient, RpcClientMock},
    Network, NetworkSpec, NetworkStatus,
};

use crate::utils::{managed_labels, Context};

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
// Add test specific implementation to the Context
impl<R> Context<R, StepRng>
where
    R: IpfsRpcClient,
{
    // Create a test context with a mocked kube and rpc clients
    pub fn test(mock_rpc_client: R) -> (Arc<Self>, ApiServerVerifier) {
        let (mock_service, handle) =
            tower_test::mock::pair::<http::Request<Body>, http::Response<Body>>();
        let mock_k_client = Client::new(mock_service, "default");
        let ctx = Self {
            k_client: mock_k_client,
            rpc_client: mock_rpc_client,
            rng: Mutex::new(StepRng::new(29, 7)),
        };
        (Arc::new(ctx), ApiServerVerifier(handle))
    }
}

// Mock for cas peer info call that is NOT ready
pub fn mock_cas_peer_info_not_ready() -> impl Clause {
    RpcClientMock::peer_info
        .next_call(matching!(_))
        .returns(Err(anyhow!("cas-ipfs not ready")))
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
    pub postgres_auth_secret: (ExpectPatch<ExpectFile>, Secret),
    pub ceramic_admin_secret: Vec<(ExpectPatch<ExpectFile>, Option<Secret>)>,
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
            ),
            ceramic_admin_secret: vec![(
                expect_file!["./testdata/default_stubs/ceramic_admin_secret"].into(),
                Some(k8s_openapi::api::core::v1::Secret {
                    metadata: kube::core::ObjectMeta {
                        name: Some("ceramic-admin".to_owned()),
                        labels: managed_labels(),
                        ..kube::core::ObjectMeta::default()
                    },
                    ..Default::default()
                }),
            )],
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

pub async fn timeout_after_1s(handle: tokio::task::JoinHandle<()>) {
    tokio::time::timeout(std::time::Duration::from_secs(1), handle)
        .await
        .expect("timeout on mock apiserver")
        .expect("stub succeeded")
}

impl ApiServerVerifier {
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
            for step in stub.ceramic_admin_secret {
                self.handle_request_response(step.0, step.1.as_ref())
                    .await
                    .expect("ceramic-admin secret step should pass");
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

// Helper struct to assert the contents of a mock Request.
// The only purpose of this struct is its debug implementation
// to be used in expect![[]] calls.
struct Request {
    pub method: String,
    pub uri: String,
    pub headers: HeaderMap,
    pub body: Raw,
}

// Explicit Debug implementation so the fields are not marked as dead code.
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("uri", &self.uri)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .finish()
    }
}

impl Request {
    async fn from_request(request: http::Request<Body>) -> Result<Self> {
        let method = request.method().to_string();
        let uri = request.uri().to_string();
        let headers = request.headers().clone();
        let body_bytes = to_bytes(request.into_body()).await?;
        let body = if !body_bytes.is_empty() {
            let json: serde_json::Value =
                serde_json::from_slice(&body_bytes).expect("body should be JSON");
            Raw(serde_json::to_string_pretty(&json)?)
        } else {
            Raw("".to_string())
        };
        Ok(Self {
            method,
            uri,
            headers,
            body,
        })
    }
}

// Raw String the does not escape its value for debugging
struct Raw(String);
impl std::fmt::Debug for Raw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Defines a common interface for asserting an expectation.
///
/// The types expect_test::Expect, expect_test::ExpectFile and ExpectPatch all implement this trait.
trait Expectation {
    /// Make an assertion about the actual value compared to the expectation.
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug);
}

/// Defines a common interface exposing the expected data for an Expectation.
///
/// This allows ExpectPatch to be composed of either Expect or ExpectFile.
trait ExpectData {
    /// The data that is expected.
    fn data(&self) -> String;
}

/// Wraps an expectation with the ability to patch that expectation.
///
/// ```
/// let mut expect: ExpectPatch<ExpectFile> = expect_file![["path/to/large/test/fixture"]].into();
/// expect.patch(expect![[r#"
/// --- original
/// +++ modified
/// @@ -1,2 +1,3 @@
///  Small patch
///  to the larger
/// +expectation data
/// "]];
/// ```
///
/// If you find yourself manually writing the patch text,
/// instead use `UPDATE_EXPECT=1 cargo test` to update the patch text itself.
#[derive(Debug)]
pub struct ExpectPatch<E> {
    original: E,
    patch: Option<Expect>,
}

impl<E> ExpectPatch<E> {
    pub fn patch(&mut self, patch: Expect) {
        self.patch = Some(patch)
    }
}

impl From<ExpectFile> for ExpectPatch<ExpectFile> {
    fn from(value: ExpectFile) -> Self {
        Self {
            original: value,
            patch: None,
        }
    }
}

impl From<Expect> for ExpectPatch<Expect> {
    fn from(value: Expect) -> Self {
        Self {
            original: value,
            patch: None,
        }
    }
}

impl<E> Expectation for ExpectPatch<E>
where
    E: ExpectData + Expectation,
{
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug) {
        if let Some(patch) = &self.patch {
            let actual = &format!("{:#?}", actual);
            let original = self.original.data();
            // Ensure both end in a new line to avoid noisy patch data
            let original = original.trim().to_owned() + "\n";
            let actual = actual.trim().to_owned() + "\n";
            let actual_patch = diffy::create_patch(&original, &actual);
            patch.assert_eq(&format!("{actual_patch}"));
        } else {
            self.original.assert_debug_eq(actual)
        }
    }
}

impl Expectation for Expect {
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug) {
        self.assert_debug_eq(actual)
    }
}
impl ExpectData for Expect {
    fn data(&self) -> String {
        self.data().to_owned()
    }
}

impl Expectation for ExpectFile {
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug) {
        self.assert_debug_eq(actual)
    }
}
impl ExpectData for ExpectFile {
    fn data(&self) -> String {
        // This is taken from the internals of expect_file, maybe we can submit a change to expose
        // the file contents directly?
        let path_of_test_file = std::path::Path::new(self.position).parent().unwrap();
        static WORKSPACE_ROOT: once_cell::sync::OnceCell<std::path::PathBuf> =
            once_cell::sync::OnceCell::new();
        let abs_path = WORKSPACE_ROOT
            .get_or_try_init(|| {
                // Until https://github.com/rust-lang/cargo/issues/3946 is resolved, this
                // is set with a hack like https://github.com/rust-lang/cargo/issues/3946#issuecomment-973132993
                if let Ok(workspace_root) = std::env::var("CARGO_WORKSPACE_DIR") {
                    return Ok(workspace_root.into());
                }

                // If a hack isn't used, we use a heuristic to find the "top-level" workspace.
                // This fails in some cases, see https://github.com/rust-analyzer/expect-test/issues/33
                let my_manifest = std::env::var("CARGO_MANIFEST_DIR")?;
                let workspace_root = std::path::Path::new(&my_manifest)
                    .ancestors()
                    .filter(|it| it.join("Cargo.toml").exists())
                    .last()
                    .unwrap()
                    .to_path_buf();

                Ok(workspace_root)
            })
            .unwrap_or_else(|_: std::env::VarError| {
                panic!(
                    "No CARGO_MANIFEST_DIR env var and the path is relative: {}",
                    path_of_test_file.display()
                )
            })
            .join(path_of_test_file)
            .join(&self.path);

        std::fs::read_to_string(abs_path)
            .unwrap_or_default()
            .replace("\r\n", "\n")
    }
}
