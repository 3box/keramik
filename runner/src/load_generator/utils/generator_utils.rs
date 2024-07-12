use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use ceramic_http_client::CeramicHttpClient;
use crate::scenario::ceramic::Credentials;
use crate::scenario::ceramic::CeramicClient;

use super::ceramic_models_utils::CeramicModelUtil;

pub static HTTP_TIMEOUT: Duration = Duration::from_secs(5);
pub static HTTP_POOL_MAX_IDLE_PER_HOST: usize = 300;

#[derive(Clone, Debug)]
pub struct CeramicConfig {
    pub admin_cli: CeramicClient,
    pub user_cli: CeramicClient,
    pub params: CeramicScenarioParameters,
}

#[derive(Clone, Debug)]
pub struct CeramicScenarioParameters {
    pub did_type: CeramicDidType,
}

#[derive(Clone, Debug)]
pub enum CeramicDidType {
    // Fetch DID from env
    EnvInjected,
    // Generate DID from scratch
    UserGenerated,
}


impl CeramicConfig {
    pub async fn initialize_config(params: CeramicScenarioParameters) -> Result<Self> {
        let creds = Credentials::admin_from_env().await?;
        let admin_cli = CeramicHttpClient::new(creds.signer);

        let user_cli = match params.did_type {
            CeramicDidType::EnvInjected => {
                let creds = Credentials::from_env().await?;
                CeramicHttpClient::new(creds.signer)
            }
            CeramicDidType::UserGenerated => {
                let creds = Credentials::new_generate_did_key().await?;
                CeramicHttpClient::new(creds.signer)
            }
        };

        Ok(Self {
            admin_cli,
            user_cli,
            params,
        })
    }
}

/**
 * The StableLoadUser struct with an HTTP client tied to a ceramic client and a throttle rate.
 */
#[derive(Clone)]
pub struct StableLoadUser {
    /**
     * The ceramic client connected to the target peer
     */
    pub ceramic_client: CeramicClient,
    /**
     * The HTTP client to send the requests
     */
    pub http_client: Client,
    /**
     * Maximum number of requests to send per second
     */
    pub throttle_rate: Duration,
    /**
     * The base URL
     */
    pub base_url: Option<String>,
    /**
     * Methods associated with the ceramic client
     */
    pub ceramic_utils: CeramicModelUtil,
}

// Methods associated with StableLoadUser
impl StableLoadUser {

    //  TODO : Write a setup function which creates the struct by accepting a targetPeerAddress and ceramicClient and returns a StabilityTestUtils
    pub async fn setup_stability_test(
        ceramic_client: CeramicClient,
        base_url: Option<String>,
    ) -> StableLoadUser {
        let http_client = Client::builder()
        .timeout(HTTP_TIMEOUT) 
        .cookie_store(false) 
        .pool_max_idle_per_host(HTTP_POOL_MAX_IDLE_PER_HOST) 
        .build()
        .unwrap(); 

        let ceramic_utils = CeramicModelUtil {
            ceramic_client: ceramic_client.clone(),
            http_client: http_client.clone(),
            base_url: base_url.clone(),
        };

        return StableLoadUser {
            ceramic_client,
            http_client,
            throttle_rate: Duration::from_millis(100),
            base_url,
            ceramic_utils,
        };
    }
}
