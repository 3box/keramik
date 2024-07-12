use anyhow::Result;
use ceramic_http_client::{
    api::{self},
    ceramic_event::StreamId,
    ModelAccountRelation, ModelDefinition,
};
use reqwest::Client;
use crate::scenario::ceramic::models::{RandomModelInstance, SmallModel};
use crate::scenario::ceramic::CeramicClient;

#[derive(Clone, Debug)]
pub struct CeramicModelUtil {
    /**
     * The ceramic client
     */
    pub ceramic_client: CeramicClient,
    /**
     * The http client
     */
    pub http_client: Client,
    /**
     * The base URL
     */
    pub base_url: Option<String>,
}


impl CeramicModelUtil {
    /**
     * Index a model
     * 
     * @param model_id The model to index
     */
    pub async fn index_model(&self, model_id: &StreamId) -> Result<()> {
        let admin_code = self.get_admin_code().await?;
        println!("Admin code: {:?}", admin_code);
        let url = self.build_url(&self.ceramic_client.index_endpoint()).await.unwrap();
        let req = self
            .ceramic_client
            .create_index_model_request(model_id, &admin_code)
            .unwrap();
        let resp = self.http_client.post(url).json(&req).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to index model"))
        }
    }

    /**
     * Generate a random model
     * 
     * @return The stream id of the created model
     */
    pub async fn generate_random_model(&self) -> Result<StreamId, anyhow::Error> {
        let small_model =
            ModelDefinition::new::<SmallModel>("load_test_small_model", ModelAccountRelation::List)
                .unwrap();
        self.setup_model(small_model).await
    }

    /**
     * Setup a model
     * 
     * @param model The model to setup
     * @return The stream id of the created model
     */
    async fn setup_model(&self, model: ModelDefinition) -> Result<StreamId, anyhow::Error> {
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

    /**
     * Create a random model instance 
     * 
     * @param model The model which defines the schema of the model instance
     * @return The stream id of the created model instance
     */
    pub async fn create_random_mid(&self, model: &StreamId) -> Result<StreamId> {
        let data = SmallModel::random();
        return self.create_mid(model, &data).await;
    }

    /**
     * Create a model instance
     * 
     * @param model The model which defines the schema of the model instance
     * @param data The data to create
     * @return The stream id of the created model instance
     */
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

    /**
     * Get the admin code
     * 
     * @return The admin code
     */
    async fn get_admin_code(&self) -> Result<String, anyhow::Error> {
        let url = self.build_url(&self.ceramic_client.admin_code_endpoint()).await.unwrap();
        let resp = self.http_client.get(url).send().await?;
        println!("Admin code response: {:?}", &resp);
        let admin_code_resp: api::AdminCodeResponse = resp.json().await?;
        println!("Admin code response: {:?}", admin_code_resp);
        let code = &admin_code_resp.code;
        println!("Admin code: {:?}", code);
        Ok(code.to_string())
    }

    /**
     * Build a URL
     * 
     * @param path The path to build the URL from
     * @return The built URL
     */
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