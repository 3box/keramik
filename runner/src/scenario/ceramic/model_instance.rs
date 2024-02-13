use std::{str::FromStr, sync::Arc, time::Duration};

use ceramic_http_client::{
    api::{self, Pagination, StreamsResponse, StreamsResponseOrError},
    ceramic_event::StreamId,
    CeramicHttpClient, FilterQuery, ModelAccountRelation, ModelDefinition,
};
use goose::{metrics::GooseRequestMetric, prelude::*};
use redis::AsyncCommands;
use tracing::warn;

use crate::{
    goose_try,
    scenario::{
        ceramic::{
            models::{self, RandomModelInstance},
            CeramicClient, Credentials,
        },
        get_redis_client, is_goose_leader,
    },
};

use super::CeramicScenarioParameters;

const SMALL_MODEL_ID_KEY: &str = "small_model_reuse_model_id";
const LARGE_MODEL_ID_KEY: &str = "large_model_reuse_model_id";
const SMALL_MID_ID_KEY: &str = "small_model_reuse_mid_id";
const LARGE_MID_ID_KEY: &str = "large_model_reuse_mid_id";

pub(crate) async fn set_key_to_stream_id(
    conn: &mut redis::aio::Connection,
    key: &str,
    stream_id: &StreamId,
) {
    let _: () = conn.set(key, stream_id.to_string()).await.unwrap();
}

pub(crate) async fn loop_until_key_value_set(
    conn: &mut redis::aio::Connection,
    key: &str,
) -> StreamId {
    loop {
        if conn.exists(key).await.unwrap() {
            let id: String = conn.get(key).await.unwrap();
            return StreamId::from_str(&id)
                .map_err(|e| {
                    tracing::error!("invalid stream: {:?} ", e);
                    e
                })
                .unwrap();
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnvBasedConfig {
    /// The DID that can be used to create models and access the admin API
    pub admin_cli: CeramicClient,
    /// The DID that should be used to interact with ceramic (may be the same as admin DID)
    pub user_cli: CeramicClient,
    /// The redis client to use for sharing model info between users
    pub redis_cli: redis::Client,
    /// The scenario parameters
    params: CeramicScenarioParameters,
}

pub struct CeramicModelInstanceTestUser {
    /// Config that needs to exist before starting the scenario
    config: EnvBasedConfig,
    /// The ID of the small model
    pub small_model_id: StreamId,
    /// The ID of the small model instance documents set up by the test
    pub small_model_instance_ids: Vec<StreamId>,
    /// The ID of the large model
    pub large_model_id: StreamId,
    /// The ID of the large model instance documents set up by the test
    pub large_model_instance_ids: Vec<StreamId>,
}

impl CeramicModelInstanceTestUser {
    pub fn user_cli(&self) -> &CeramicClient {
        &self.config.user_cli
    }

    /// Call this before starting the scenario to verify everything is configured appropriately
    pub async fn prep_scenario(
        params: CeramicScenarioParameters,
    ) -> anyhow::Result<EnvBasedConfig> {
        let redis_cli = get_redis_client().await?;
        let creds = Credentials::from_env().await.unwrap();
        let admin_cli = CeramicHttpClient::new(creds.signer);

        let user_cli = if params.did_type == crate::scenario::ceramic::DidType::UserDidKey {
            let creds = Credentials::new_generate_did_key().await?;
            CeramicHttpClient::new(creds.signer)
        } else {
            admin_cli.clone()
        };

        Ok(EnvBasedConfig {
            admin_cli,
            user_cli,
            redis_cli,
            params,
        })
    }
    /// Builds the CeramicLoadTestUser and stores it as user session data
    pub async fn setup_scenario(
        user: &mut GooseUser,
        config: EnvBasedConfig,
    ) -> anyhow::Result<()> {
        let (small_model_id, large_model_id) = match config.params.model_reuse {
            super::ReuseType::PerUser => {
                Self::generate_list_models(user, &config.admin_cli).await?
            }
            super::ReuseType::Shared => {
                let mut conn = config.redis_cli.get_async_connection().await.unwrap();
                if is_goose_leader() {
                    let (small, large) =
                        Self::generate_list_models(user, &config.admin_cli).await?;
                    ModelInstanceRequests::index_model(user, &config.admin_cli, &small, "small")
                        .await?;
                    ModelInstanceRequests::index_model(user, &config.admin_cli, &large, "large")
                        .await?;
                    let _ = set_key_to_stream_id(&mut conn, SMALL_MODEL_ID_KEY, &small).await;
                    let _ = set_key_to_stream_id(&mut conn, LARGE_MODEL_ID_KEY, &large).await;

                    (small, large)
                } else {
                    let small = loop_until_key_value_set(&mut conn, SMALL_MODEL_ID_KEY).await;
                    let large = loop_until_key_value_set(&mut conn, LARGE_MODEL_ID_KEY).await;
                    (small, large)
                }
            }
        };

        let (small_model_instance_ids, large_model_instance_ids) = match config
            .params
            .model_instance_reuse
        {
            super::ReuseType::PerUser => {
                Self::generate_mids(
                    user,
                    &config.user_cli,
                    &small_model_id,
                    &large_model_id,
                    config.params.number_of_documents,
                )
                .await?
            }
            super::ReuseType::Shared => {
                if config.params.number_of_documents != 1 {
                    warn!("Shared model instance reuse only supports 1 document per model currently. Only using the first document for each model.");
                }
                let mut conn = config.redis_cli.get_async_connection().await.unwrap();
                if is_goose_leader() {
                    let (small, large) = Self::generate_mids(
                        user,
                        &config.user_cli,
                        &small_model_id,
                        &large_model_id,
                        config.params.number_of_documents,
                    )
                    .await?;
                    let small_mid = small.first().unwrap();
                    let large_mid = large.first().unwrap();
                    let _ = set_key_to_stream_id(&mut conn, SMALL_MID_ID_KEY, small_mid).await;
                    let _ = set_key_to_stream_id(&mut conn, LARGE_MID_ID_KEY, large_mid).await;

                    (small, large)
                } else {
                    let small = loop_until_key_value_set(&mut conn, SMALL_MID_ID_KEY).await;
                    let large = loop_until_key_value_set(&mut conn, LARGE_MID_ID_KEY).await;
                    (vec![small], vec![large])
                }
            }
        };

        let resp = Self {
            config,
            small_model_id,
            small_model_instance_ids,
            large_model_id,
            large_model_instance_ids,
        };

        user.set_session_data(Arc::new(resp));
        Ok(())
    }

    /// This is awkward. But as we set ourself up, we should be able to guanratee that the user data is set.
    /// We use an Arc as we want to allow borrowing this 'context' data, while taking a separate &mut GooseUser
    /// request. It would be possible to do this, and match on which "type" of stream update we're making, but
    /// it's nice to accept all the stream IDs etc as parameters to functions.
    pub fn user_data(user: &GooseUser) -> &Arc<Self> {
        user.get_session_data_unchecked()
    }

    pub fn _random_model_for_user(&self, user: &GooseUser) -> &StreamId {
        let idx = user.weighted_users_index % self.large_model_instance_ids.len();
        let val = self.large_model_instance_ids.get(idx);
        // we should always have a value, since the index is always less than the length
        val.unwrap()
    }

    /// returns (small, large) model IDs
    async fn generate_list_models(
        user: &mut GooseUser,
        admin_cli: &CeramicClient,
    ) -> Result<(StreamId, StreamId), TransactionError> {
        let small_model = ModelDefinition::new::<models::SmallModel>(
            "load_test_small_model",
            ModelAccountRelation::List,
        )
        .unwrap();
        let small_model_id =
            ModelInstanceRequests::setup_model(user, admin_cli, small_model, "small").await?;

        let large_model = ModelDefinition::new::<models::LargeModel>(
            "load_test_large_model",
            ModelAccountRelation::List,
        )
        .unwrap();
        let large_model_id =
            ModelInstanceRequests::setup_model(user, admin_cli, large_model, "large").await?;

        Ok((small_model_id, large_model_id))
    }

    async fn generate_mids(
        user: &mut GooseUser,
        cli: &CeramicClient,
        small_model_id: &StreamId,
        large_model_id: &StreamId,
        number_of_documents: usize,
    ) -> Result<(Vec<StreamId>, Vec<StreamId>), TransactionError> {
        let doc_cnt = if number_of_documents == 0 {
            1
        } else {
            number_of_documents
        };
        let mut small_model_instance_ids = vec![];
        let mut large_model_instance_ids = vec![];
        for i in 0..doc_cnt {
            let small_model_instance_id = ModelInstanceRequests::create_model_instance(
                user,
                cli,
                small_model_id,
                "small",
                &models::SmallModel::random(),
            )
            .await?;
            let large_model_instance_id = ModelInstanceRequests::create_model_instance(
                user,
                cli,
                large_model_id,
                "large",
                // the following values are used to make assumptions about the values set in the model instance documents during the "query" test currently
                &models::LargeModel::new(
                    format!("large_model_{}", i),
                    i.to_string(),
                    10 + (i * 10) as i64,
                ),
            )
            .await?;
            small_model_instance_ids.push(small_model_instance_id);
            large_model_instance_ids.push(large_model_instance_id);
        }

        Ok((small_model_instance_ids, large_model_instance_ids))
    }
}

#[derive(Clone, Debug)]
pub struct ModelInstanceRequests {}

impl ModelInstanceRequests {
    pub async fn get_stream_tx(
        user: &mut GooseUser,
        cli: &CeramicClient,
        stream_id: &StreamId,
        tx_name: &str,
    ) -> TransactionResult {
        let _goose = Self::get_stream(user, cli, stream_id, tx_name).await?;
        Ok(())
    }

    pub async fn get_and_replace_stream_tx<T>(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model_id: &StreamId,
        model_instance_id: &StreamId,
        tx_name: &str,
        data: &T,
    ) -> TransactionResult
    where
        T: serde::Serialize,
    {
        let (resp, _) = Self::get_stream(user, cli, model_instance_id, tx_name).await?;
        Self::replace_stream_tx(user, cli, model_id, &resp, tx_name, data).await?;
        Ok(())
    }

    pub async fn replace_stream_tx<T>(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model_id: &StreamId,
        prev: &StreamsResponse,
        tx_name: &str,
        data: &T,
    ) -> TransactionResult
    where
        T: serde::Serialize,
    {
        let update_req = cli
            .create_replace_request(model_id, prev, data)
            .await
            .unwrap();
        let commits_url = user.build_url(cli.commits_endpoint())?;

        let req: reqwest::RequestBuilder = user.client.post(commits_url).json(&update_req);
        let name = format!("Replace_{}", tx_name);
        let req = GooseRequest::builder()
            .name(name.as_str())
            .method(GooseMethod::Post)
            .set_request_builder(req)
            .expect_status_code(200)
            .build();
        let mut goose = user.request(req).await?;
        let resp: StreamsResponseOrError = goose.response?.json().await?;
        goose_try!(user, &name, &mut goose.request, resp.resolve(tx_name))?;
        Ok(())
    }

    pub async fn query_model(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model_id: &StreamId,
        filter: Option<FilterQuery>,
    ) -> Result<(api::QueryResponse, GooseRequestMetric), TransactionError> {
        let req = cli
            .create_query_request(model_id, filter, Pagination::default())
            .await
            .unwrap();
        let goose = user
            .request(
                GooseRequest::builder()
                    .name("query_model")
                    .method(GooseMethod::Post)
                    .set_request_builder(
                        user.client
                            .post(user.build_url(cli.collection_endpoint())?)
                            .json(&req),
                    )
                    .expect_status_code(200)
                    .build(),
            )
            .await?;
        let resp: api::QueryResponse = goose.response?.json().await?;

        Ok((resp, goose.request))
    }

    pub async fn get_stream(
        user: &mut GooseUser,
        cli: &CeramicClient,
        stream_id: &StreamId,
        tx_name: &str,
    ) -> Result<(StreamsResponse, GooseRequestMetric), TransactionError> {
        let streams_url = user.build_url(&format!("{}/{}", cli.streams_endpoint(), stream_id))?;
        let name = format!("Get_{}", tx_name);
        let get_stream_req = GooseRequest::builder()
            .name(name.as_str())
            .method(GooseMethod::Get)
            .set_request_builder(user.client.get(streams_url))
            .expect_status_code(200)
            .build();

        let mut goose = user.request(get_stream_req).await?;
        // let mut goose = Self::get_model(user, cli, model_id, tx_name).await?;
        let resp: StreamsResponseOrError = goose.response?.json().await?;

        let resp = goose_try!(user, &name, &mut goose.request, { resp.resolve(tx_name) })?;

        Ok((resp, goose.request))
    }

    pub async fn setup_model(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model: ModelDefinition,
        tx_name: &str,
    ) -> Result<StreamId, TransactionError> {
        let url = user.build_url(cli.streams_endpoint())?;
        let req = cli.create_model_request(&model).await.unwrap();
        let req = user.client.post(url).json(&req);
        let name = format!("setup_model_{}", tx_name);
        let req = GooseRequest::builder()
            .name(name.as_str())
            .method(GooseMethod::Post)
            .set_request_builder(req)
            .expect_status_code(200)
            .build();
        let mut goose = user.request(req).await?;
        let resp: api::StreamsResponseOrError = goose.response?.json().await?;
        let resp = goose_try!(user, &name, &mut goose.request, { resp.resolve(&name) })?;
        Ok(resp.stream_id)
    }

    pub async fn create_model_instance<T: serde::Serialize>(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model: &StreamId,
        tx_name: &str,
        data: &T,
    ) -> Result<StreamId, TransactionError> {
        let url = user.build_url(cli.streams_endpoint())?;
        let req = cli.create_list_instance_request(model, data).await.unwrap();
        let req = user.client.post(url).json(&req);
        let name = format!("create_model_instance_{}", tx_name);
        let req = GooseRequest::builder()
            .name(name.as_str())
            .method(GooseMethod::Post)
            .set_request_builder(req)
            .expect_status_code(200)
            .build();
        let mut goose = user.request(req).await?;
        let resp: api::StreamsResponseOrError = goose.response?.json().await?;
        let resp = goose_try!(user, &name, &mut goose.request, { resp.resolve(tx_name) })?;
        Ok(resp.stream_id)
    }

    pub async fn index_model(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model_id: &StreamId,
        tx_name: &str,
    ) -> Result<(), TransactionError> {
        let name = format!("admin_code_{}", tx_name);
        let result = user
            .request(
                GooseRequest::builder()
                    .name(name.as_str())
                    .method(GooseMethod::Get)
                    .set_request_builder(
                        user.client.get(user.build_url(cli.admin_code_endpoint())?),
                    )
                    .expect_status_code(200)
                    .build(),
            )
            .await?;
        let resp: api::AdminCodeResponse = result.response?.json().await?;
        let name = format!("create_index_model_{}", tx_name);
        let req = cli
            .create_index_model_request(model_id, &resp.code)
            .await
            .unwrap();
        let mut goose = user
            .request(
                GooseRequest::builder()
                    .name(name.as_str())
                    .method(GooseMethod::Post)
                    .set_request_builder(
                        user.client
                            .post(user.build_url(cli.index_endpoint())?)
                            .json(&req),
                    )
                    .expect_status_code(200)
                    .build(),
            )
            .await?;
        let resp = goose.response?;
        if resp.status().is_success() {
            Ok(())
        } else {
            user.set_failure(
                &format!("index_model_{}", name),
                &mut goose.request,
                None,
                Some(&format!("Failed to index model: {}", resp.text().await?)),
            )
        }
    }
}
