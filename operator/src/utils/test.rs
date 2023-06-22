use std::sync::{Arc, Mutex};

use anyhow::Result;
use hyper::{body::to_bytes, Body};
use kube::Client;
use rand::rngs::mock::StepRng;
use reqwest::header::HeaderMap;

use crate::{network::utils::RpcClient, utils::Context};

pub type ApiServerHandle = tower_test::mock::Handle<http::Request<Body>, http::Response<Body>>;

// Add test specific implementation to the Context
impl<R> Context<R, StepRng>
where
    R: RpcClient,
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

pub async fn timeout_after_1s(handle: tokio::task::JoinHandle<()>) {
    tokio::time::timeout(std::time::Duration::from_secs(1), handle)
        .await
        .expect("timeout on mock apiserver")
        .expect("stub succeeded")
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
