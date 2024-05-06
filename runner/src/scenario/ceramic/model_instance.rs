use std::{str::FromStr, sync::Arc, time::Duration};

use ceramic_http_client::{
    api::{self, Pagination, StreamsResponse, StreamsResponseOrError},
    ceramic_event::StreamId,
    CeramicHttpClient, FilterQuery, ModelAccountRelation, ModelDefinition,
};
use goose::{metrics::GooseRequestMetric, prelude::*};
use redis::AsyncCommands;
use tracing::{debug, info, warn};

use crate::{
    goose_try,
    scenario::{
        ceramic::{
            models::{self, RandomModelInstance},
            CeramicClient, Credentials,
        },
        get_redis_client, is_goose_global_leader, is_goose_lead_user, is_goose_lead_worker,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CountResponse {
    pub count: i32,
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

#[derive(Clone, Debug)]
pub struct GooseUserInfo {
    pub global_leader: bool,
    /// True if this user is the lead user on the worker
    pub lead_user: bool,
    /// True if this is the lead worker process
    pub lead_worker: bool,
}

#[derive(Clone, Debug)]
pub struct CeramicModelInstanceTestUser {
    /// Config that needs to exist before starting the scenario
    config: EnvBasedConfig,
    /// True if this user is the global leader
    pub user_info: GooseUserInfo,
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

    pub fn redis_cli(&self) -> &redis::Client {
        &self.config.redis_cli
    }

    /// Call this before starting the scenario to verify everything is configured appropriately
    /// It could be in the `setup_scenario` function, but it's "nice" to crash the worker rather than
    /// only error in the setup function, which is harder to notice right away.
    pub async fn prep_scenario(
        params: CeramicScenarioParameters,
    ) -> anyhow::Result<EnvBasedConfig> {
        let redis_cli = get_redis_client().await?;
        let creds = Credentials::admin_from_env().await?;
        let admin_cli = CeramicHttpClient::new(creds.signer);

        let user_cli = match params.did_type {
            super::DidType::Shared => {
                let creds = Credentials::from_env().await?;
                CeramicHttpClient::new(creds.signer)
            }
            super::DidType::UserDidKey => {
                let creds = Credentials::new_generate_did_key().await?;
                CeramicHttpClient::new(creds.signer)
            }
        };

        Ok(EnvBasedConfig {
            admin_cli,
            user_cli,
            redis_cli,
            params,
        })
    }

    pub async fn setup_mid_scenario(
        user: &mut GooseUser,
        config: EnvBasedConfig,
    ) -> TransactionResult {
        Self::setup_scenario(user, config)
            .await
            .map_err(|e| {
                tracing::error!("failed to setup scenario: {}", e);
                e
            })
            .unwrap();
        Ok(())
    }
    /// Builds the CeramicLoadTestUser and stores it as user session data
    async fn setup_scenario(user: &mut GooseUser, config: EnvBasedConfig) -> anyhow::Result<()> {
        let lead_user = is_goose_lead_user(); // we cache this as the implementation is not idempotent
        let global_leader = is_goose_global_leader(lead_user);
        debug!(params=?config.params, "setting up scenario");
        let (small_model_id, large_model_id) = match config.params.model_reuse {
            super::ReuseType::PerUser => {
                Self::generate_list_models(
                    user,
                    &config.admin_cli,
                    &config.redis_cli,
                    true,
                    None,
                    false,
                )
                .await?
            }
            super::ReuseType::Shared => {
                let (small, large) = Self::generate_list_models(
                    user,
                    &config.admin_cli,
                    &config.redis_cli,
                    global_leader,
                    None,
                    false,
                )
                .await?;
                // js ceramic subscribes to the meta model, so we'll get all models created synced to us. we just need to make sure they sync before starting
                Self::ensure_model_exists(user, &config.user_cli, &small).await?;
                Self::ensure_model_exists(user, &config.user_cli, &large).await?;
                (small, large)
            }
            crate::scenario::ceramic::ReuseType::PerNode => {
                // we need to adjust the redis key when we want to avoid overwriting the same key from different nodes. usually we want to share (empty),
                // but in this case we want to only share for a single worker (all users)
                Self::generate_list_models(
                    user,
                    &config.admin_cli,
                    &config.redis_cli,
                    lead_user,
                    Some(goose::get_worker_id().to_string()),
                    false,
                )
                .await?
            }
            crate::scenario::ceramic::ReuseType::LeadWorkerSubscriber => {
                let (small, large) = Self::generate_list_models(
                    user,
                    &config.admin_cli,
                    &config.redis_cli,
                    global_leader,
                    None,
                    true,
                )
                .await?;
                Self::ensure_model_exists(user, &config.user_cli, &small).await?;
                Self::ensure_model_exists(user, &config.user_cli, &large).await?;
                (small, large)
            }
        };

        if lead_user {
            ModelInstanceRequests::index_model(
                user,
                &config.admin_cli,
                &small_model_id,
                "index_small",
            )
            .await?;
            ModelInstanceRequests::index_model(
                user,
                &config.admin_cli,
                &large_model_id,
                "index_large",
            )
            .await?;

            Self::subscribe_to_model(user, &small_model_id).await?;
            Self::subscribe_to_model(user, &large_model_id).await?;
        }

        let (small_model_instance_ids, large_model_instance_ids) = match config
            .params
            .model_instance_reuse
        {
            super::ReuseType::PerUser => {
                Self::generate_mids(
                    user,
                    &config.user_cli,
                    &config.redis_cli,
                    &small_model_id,
                    &large_model_id,
                    config.params.number_of_documents,
                    true,
                    None,
                )
                .await?
            }
            super::ReuseType::PerNode => {
                Self::generate_mids(
                    user,
                    &config.user_cli,
                    &config.redis_cli,
                    &small_model_id,
                    &large_model_id,
                    config.params.number_of_documents,
                    lead_user,
                    Some(goose::get_worker_id().to_string()),
                )
                .await?
            }
            super::ReuseType::Shared => {
                if config.params.number_of_documents != 1 {
                    warn!("Shared model instance reuse only supports 1 document per model currently. Only using the first document for each model.");
                }
                Self::generate_mids(
                    user,
                    &config.user_cli,
                    &config.redis_cli,
                    &small_model_id,
                    &large_model_id,
                    config.params.number_of_documents,
                    global_leader,
                    None,
                )
                .await?
            }
            super::ReuseType::LeadWorkerSubscriber => {
                // For model instance reuse type make it work the same way as shared
                Self::generate_mids(
                    user,
                    &config.user_cli,
                    &config.redis_cli,
                    &small_model_id,
                    &large_model_id,
                    config.params.number_of_documents,
                    global_leader,
                    None,
                )
                .await?
            }
        };

        let resp = Self {
            config,
            user_info: GooseUserInfo {
                lead_user,
                global_leader,
                lead_worker: is_goose_lead_worker(),
            },
            small_model_id,
            small_model_instance_ids,
            large_model_id,
            large_model_instance_ids,
        };

        user.set_session_data(Arc::new(resp));
        info!("scenario setup complete");
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
        redis_cli: &redis::Client,
        should_create: bool,
        redis_postfix: Option<String>,
        should_subscribe: bool,
    ) -> Result<(StreamId, StreamId), TransactionError> {
        let mut conn = redis_cli.get_async_connection().await.unwrap();
        let (small_key, large_key) = if let Some(pf) = redis_postfix {
            (
                format!("{}_{}", SMALL_MODEL_ID_KEY, pf),
                format!("{}_{}", LARGE_MODEL_ID_KEY, pf),
            )
        } else {
            (
                SMALL_MODEL_ID_KEY.to_string(),
                LARGE_MODEL_ID_KEY.to_string(),
            )
        };
        if should_create {
            info!("generating model IDs");
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

            let _ = set_key_to_stream_id(&mut conn, &small_key, &small_model_id).await;
            let _ = set_key_to_stream_id(&mut conn, &large_key, &large_model_id).await;

            Ok((small_model_id, large_model_id))
        } else {
            info!("waiting for shared model IDs to be set in redis");
            let small = loop_until_key_value_set(&mut conn, &small_key).await;
            let large = loop_until_key_value_set(&mut conn, &large_key).await;
            if should_subscribe {
                Self::subscribe_to_model(user, &small).await?;
                Self::subscribe_to_model(user, &large).await?;
            }
            Ok((small, large))
        }
    }

    async fn subscribe_to_model(user: &mut GooseUser, model_id: &StreamId) -> TransactionResult {
        let request_builder = user
            .get_request_builder(
                &GooseMethod::Post,
                &format!("/ceramic/interests/model/{}", model_id),
            )?
            .timeout(Duration::from_secs(5));
        let req = GooseRequest::builder()
            .set_request_builder(request_builder)
            .expect_status_code(204)
            .build();
        let _goose = user.request(req).await?;
        Ok(())
    }

    async fn ensure_model_exists(
        user: &mut GooseUser,
        cli: &CeramicClient,
        model_id: &StreamId,
    ) -> anyhow::Result<()> {
        let now = std::time::SystemTime::now();
        loop {
            match ModelInstanceRequests::get_stream_int(
                user,
                cli,
                model_id,
                "ensure_model_exists",
                false,
            )
            .await
            {
                Ok((r, _)) => {
                    info!("got response: {:?}", r);
                    if r.resolve("ensure_model_exists").is_ok() {
                        info!(
                            "model {} exists after {} seconds",
                            model_id,
                            now.elapsed().unwrap().as_secs()
                        );
                        break;
                    }
                }
                Err(e) => {
                    info!("failed to get model: {:?}", e);
                }
            }
            if now.elapsed().unwrap() > Duration::from_secs(60) {
                anyhow::bail!("timed out waiting for model to exist: {}", model_id);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn generate_mids(
        user: &mut GooseUser,
        cli: &CeramicClient,
        redis_cli: &redis::Client,
        small_model_id: &StreamId,
        large_model_id: &StreamId,
        number_of_documents: usize,
        should_create: bool,
        redis_postfix: Option<String>,
    ) -> Result<(Vec<StreamId>, Vec<StreamId>), TransactionError> {
        if number_of_documents == 0 {
            // not all scenarios care to prep documents in advance
            return Ok((vec![], vec![]));
        }

        let (small_key, large_key) = if let Some(pf) = redis_postfix {
            (
                format!("{}_{}", SMALL_MID_ID_KEY, pf),
                format!("{}_{}", LARGE_MID_ID_KEY, pf),
            )
        } else {
            (SMALL_MID_ID_KEY.to_string(), LARGE_MID_ID_KEY.to_string())
        };

        let mut conn = redis_cli.get_async_connection().await.unwrap();
        if should_create {
            info!("generating {} model instance IDs", number_of_documents);

            let mut small_model_instance_ids = vec![];
            let mut large_model_instance_ids = vec![];
            for i in 0..number_of_documents {
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
                // we only support one shared ID for now, so just add the first one we create
                if i == 0 {
                    let _ =
                        set_key_to_stream_id(&mut conn, &small_key, &small_model_instance_id).await;
                    let _ =
                        set_key_to_stream_id(&mut conn, &large_key, &large_model_instance_id).await;
                }
                small_model_instance_ids.push(small_model_instance_id);
                large_model_instance_ids.push(large_model_instance_id);
            }

            Ok((small_model_instance_ids, large_model_instance_ids))
        } else {
            info!("waiting for shared model instance IDs");

            let small = loop_until_key_value_set(&mut conn, &small_key).await;
            let large = loop_until_key_value_set(&mut conn, &large_key).await;
            Ok((vec![small], vec![large]))
        }
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
        let goose: goose::goose::GooseResponse = user
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

    #[allow(dead_code)]
    pub async fn query_model_count(
        user: &mut GooseUser,
        model_id: &StreamId,
    ) -> Result<(CountResponse, GooseRequestMetric), TransactionError> {
        let req = serde_json::json!({
            "model": model_id.to_string(),
        });
        let goose = user
            .request(
                GooseRequest::builder()
                    .name("query_model_count")
                    .method(GooseMethod::Post)
                    .set_request_builder(
                        user.client
                            .post(user.build_url("/api/v0/collection/count")?)
                            .json(&req),
                    )
                    .expect_status_code(200)
                    .build(),
            )
            .await?;
        let resp: CountResponse = goose.response?.json().await?;

        Ok((resp, goose.request))
    }

    pub async fn get_stream_int(
        user: &mut GooseUser,
        cli: &CeramicClient,
        stream_id: &StreamId,
        name: &str,
        expect_success: bool,
    ) -> Result<(StreamsResponseOrError, GooseRequestMetric), TransactionError> {
        let streams_url = user.build_url(&format!("{}/{}", cli.streams_endpoint(), stream_id))?;
        let get_stream_req = GooseRequest::builder()
            .name(name)
            .method(GooseMethod::Get)
            .set_request_builder(user.client.get(streams_url))
            .expect_status_code(200)
            .build();

        // unfortunately goose logs an error if it's a 500, so we expect 500 to avoid some noised
        let mut goose = user.request(get_stream_req).await?;
        let resp: StreamsResponseOrError = goose.response?.json().await?;
        if !expect_success {
            user.set_success(&mut goose.request)?;
        }

        Ok((resp, goose.request))
    }

    pub async fn get_stream(
        user: &mut GooseUser,
        cli: &CeramicClient,
        stream_id: &StreamId,
        tx_name: &str,
    ) -> Result<(StreamsResponse, GooseRequestMetric), TransactionError> {
        let name = format!("Get_{}", tx_name);
        let (resp, mut goose) = Self::get_stream_int(user, cli, stream_id, &name, true).await?;
        let resp = goose_try!(user, &name, &mut goose, { resp.resolve(tx_name) })?;
        Ok((resp, goose))
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
