use crate::scenario::ceramic::CeramicClient;
use crate::scenario::ceramic::Credentials;
use anyhow::Result;
use ceramic_http_client::CeramicHttpClient;
use reqwest::Client;
use std::time::Duration;

use super::ceramic_models_utils::CeramicModelUser;

pub static HTTP_TIMEOUT: Duration = Duration::from_secs(5);
pub static HTTP_POOL_MAX_IDLE_PER_HOST: usize = 300;

#[derive(Clone, Debug)]
pub struct CeramicConfig {
    /// Client with admin API permission
    pub admin_cli: CeramicClient,
    #[allow(dead_code)]
    /// Client without write permission and admin API permission
    pub user_cli: CeramicClient,
    /// Parameters for the scenario
    #[allow(dead_code)]
    pub params: CeramicScenarioParameters,
}

#[derive(Clone, Debug)]
pub struct CeramicScenarioParameters {
    /// Type of DID to use
    pub did_type: CeramicDidType,
}

#[derive(Clone, Debug)]
pub enum CeramicDidType {
    // Fetch DID from env
    EnvInjected,
    // Generate DID from scratch
    #[allow(dead_code)]
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
     * Maximum number of requests to send per second
     */
    #[allow(dead_code)]
    pub throttle_rate: Duration,
    /**
     * Methods associated with the ceramic client
     */
    pub ceramic_utils: CeramicModelUser,
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

        let ceramic_utils = CeramicModelUser {
            ceramic_client: ceramic_client.clone(),
            http_client: http_client.clone(),
            base_url: base_url.clone(),
        };

        StableLoadUser {
            throttle_rate: Duration::from_millis(100),
            ceramic_utils,
        }
    }
}
