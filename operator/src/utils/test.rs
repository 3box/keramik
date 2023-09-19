//! Basic implementation of an API server verifier.
use std::sync::{Arc, Mutex};

use anyhow::Result;
use expect_patch::Expectation;
use hyper::{body::to_bytes, Body};
use kube::{error::ErrorResponse, Client};
use rand::rngs::mock::StepRng;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::{network::ipfs_rpc::IpfsRpcClient, utils::Context};

pub type ApiServerHandle = tower_test::mock::Handle<http::Request<Body>, http::Response<Body>>;

// Add test specific implementation to the Context
impl<R> Context<R, StepRng>
where
    R: IpfsRpcClient,
{
    // Create a test context with a mocked kube and rpc clients
    pub fn test(mock_rpc_client: R) -> (Arc<Self>, ApiServerHandle) {
        let (mock_service, handle) =
            tower_test::mock::pair::<http::Request<Body>, http::Response<Body>>();
        let mock_k_client = Client::new(mock_service, "default");
        let ctx = Self {
            k_client: mock_k_client,
            rpc_client: mock_rpc_client,
            rng: Mutex::new(StepRng::new(29, 7)),
        };
        (Arc::new(ctx), handle)
    }
}

pub async fn timeout_after_1s<T>(handle: tokio::task::JoinHandle<T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(1), handle)
        .await
        .expect("timeout on mock apiserver")
        .expect("stub succeeded")
}

/// ApiServerVerifier verifies that the serve is called according to test expectations.
pub struct ApiServerVerifier(ApiServerHandle);

pub trait WithStatus {
    type Status;
    fn with_status(self, status: Self::Status) -> Self;
}

impl ApiServerVerifier {
    /// Create an ApiServerVerifier from a handle
    pub fn new(handle: ApiServerHandle) -> Self {
        Self(handle)
    }

    pub async fn handle_patch_status<R>(
        &mut self,
        expected_request: impl Expectation,
        resource: R,
    ) -> Result<R>
    where
        R: WithStatus + Serialize,
        <R as WithStatus>::Status: for<'de> Deserialize<'de>,
    {
        let (request, send) = self.0.next_request().await.expect("service not called");
        let request = Request::from_request(request).await?;
        expected_request.assert_debug_eq(&request);

        let json: serde_json::Value =
            serde_json::from_str(&request.body.0).expect("status should be JSON");

        let status_json = json.get("status").expect("status object").clone();
        let status: <R as WithStatus>::Status =
            serde_json::from_value(status_json).expect("JSON should be a valid status");

        let resource = resource.with_status(status);
        let response = serde_json::to_vec(&resource).unwrap();
        send.send_response(
            http::Response::builder()
                .body(Body::from(response))
                .unwrap(),
        );
        Ok(resource)
    }

    pub async fn handle_apply(&mut self, expected_request: impl Expectation) -> Result<()> {
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
    pub async fn handle_request_response<T>(
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

/// Helper struct to assert the contents of a mock Request.
/// The only purpose of this struct is its debug implementation
/// to be used in expect![[]] calls.
pub struct Request {
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
    pub async fn from_request(request: http::Request<Body>) -> Result<Self> {
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
pub struct Raw(pub String);

impl std::fmt::Debug for Raw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
