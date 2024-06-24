use anyhow::Result;
use ceramic_http_client::{
    api::{self},
    ceramic_event::StreamId,
    ModelAccountRelation, ModelDefinition,
};
use reqwest::Client;
use std::time::Duration;
// use goose::{metrics::GooseRequestMetric, prelude::*};
// use redis::AsyncCommands;
use super::ceramic::models::RandomModelInstance;
use crate::scenario::ceramic::models::SmallModel;
use crate::scenario::ceramic::CeramicClient;
use tracing::info;

// Define the StableLoadUser struct with an HTTP client and a throttle rate.

#[derive(Clone)]
pub struct StableLoadUser {
    pub ceramic_client: CeramicClient,
    pub http_client: Client,
    pub throttle_rate: Duration,
    pub base_url: Option<String>,
}

// Methods associated with StableLoadUser
impl StableLoadUser {

    async fn index_model(&self, model_id: &StreamId) -> Result<()> {
        let url = self.ceramic_client.admin_code_endpoint();
        let response = self.http_client.get(url).send().await?;
        let resp: api::AdminCodeResponse = response.json().await?;
        let req = self
            .ceramic_client
            .create_index_model_request(model_id, &resp.code)
            .await
            .unwrap();
        let resp = self.http_client.post(url).json(&req).send().await?;

        let admin_resp: api::AdminCodeResponse = resp.json().await?;
        let url = self.ceramic_client.index_endpoint();
        let create_index_req = self
            .ceramic_client
            .create_index_model_request(model_id, &admin_resp.code)
            .await
            .unwrap();
        let resp = self.http_client.post(url).json(&create_index_req).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to index model"))
        }
    }

    pub async fn generate_random_model(&self) -> Result<StreamId, anyhow::Error> {
        let small_model =
            ModelDefinition::new::<SmallModel>("load_test_small_model", ModelAccountRelation::List)
                .unwrap();
        self.setup_model(small_model).await
    }

    async fn setup_model(&self, model: ModelDefinition) -> Result<StreamId, anyhow::Error> {
        // I want the url to look something like https://localhost:<ceramic-port> + models endpoint + the url should have a base
        let url = self
            .build_url(&self.ceramic_client.streams_endpoint())
            .await
            .unwrap();
        info!("URL: {}", url);
        let req = self.ceramic_client.create_model_request(&model).await.unwrap();
        let req = self.http_client.post(url).json(&req);
        let resp: reqwest::Response = req.send().await?;
        if resp.status() == reqwest::StatusCode::OK {
            let streams_response: api::StreamsResponse = resp.json().await?;
            info!("Stream ID: {:?}", streams_response.stream_id);
            Ok(streams_response.stream_id)
        } else {
            Err(anyhow::anyhow!(
                "Failed to setup model: status {:?} , resp_text {:?}",
                resp.status(),
                resp.text().await
            ))
        }
    }
    //  TODO : Write a setup function which creates the struct by accepting a targetPeerAddress and ceramicClient and returns a StabilityTestUtils
    pub async fn setup_stability_test(
        ceramic_client: CeramicClient,
        base_url: Option<String>,
    ) -> StableLoadUser {
        let http_client = Client::new();
        return StableLoadUser {
            ceramic_client,
            http_client,
            throttle_rate: Duration::from_millis(100),
            base_url,
        };
    }

    pub async fn create_random_mid(&self, model: &StreamId) -> Result<StreamId> {
        let data = SmallModel::random();
        return self.create_mid(model, &data).await;
    }

    async fn create_mid(&self, model: &StreamId, data: &SmallModel) -> Result<StreamId> {
        let url = self
            .build_url(&self.ceramic_client.streams_endpoint())
            .await
            .unwrap();
        let req = self
            .ceramic_client
            .create_list_instance_request(model, data)
            .await
            .unwrap();
        let req = self.http_client.post(url).json(&req);
        let resp: reqwest::Response = req.send().await?;
        if resp.status() == reqwest::StatusCode::OK {
            let parsed_resp: api::StreamsResponse = resp.json().await?;
            Ok(parsed_resp.stream_id)
        } else {
            Err(anyhow::anyhow!(
                "Failed to create model: status {:?} , resp_text {:?}",
                resp.status(),
                resp.text().await
            ))
        }
    }

    async fn build_url(&self, path: &str) -> Result<String, anyhow::Error> {
        let base = self
            .base_url
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Base URL is not set"))?;
        let separator = if path.starts_with('/') || base.ends_with('/') {
            ""
        } else {
            "/"
        };
        let full_url = format!("{}{}{}", base, separator, path);
        Ok(full_url)
    }
}

