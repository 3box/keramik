mod models;
pub mod new_streams;
pub mod write_only;

use ceramic_http_client::api::StreamsResponseOrError;
use ceramic_http_client::ceramic_event::{DidDocument, StreamId};
use ceramic_http_client::{CeramicHttpClient, ModelAccountRelation, ModelDefinition};
use goose::prelude::*;
use models::RandomModelInstance;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tracing::{debug, instrument};

#[derive(Clone)]
pub struct Credentials {
    pub signer: DidDocument,
    pub private_key: String,
}

impl Credentials {
    fn new() -> Self {
        let signer = DidDocument::new(&std::env::var("DID_KEY").unwrap());
        let private_key = std::env::var("DID_PRIVATE_KEY").unwrap();
        Self {
            signer,
            private_key,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoadTestUserData {
    cli: CeramicHttpClient,
    small_model_id: StreamId,
    small_model_instance_id: StreamId,
    large_model_id: StreamId,
    large_model_instance_id: StreamId,
}

pub fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::new();
    let cli = CeramicHttpClient::new(creds.signer, &creds.private_key);

    let setup_cli = cli;
    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, setup_cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();

    let update_small_model = transaction!(update_small_model).set_name("update_small_model");

    let get_small_model = transaction!(get_small_model).set_name("get_small_model");

    let update_large_model = transaction!(update_large_model).set_name("update_large_model");

    let get_large_model = transaction!(get_large_model).set_name("get_large_model");

    Ok(scenario!("CeramicSimpleScenario")
        // After each transactions runs, sleep randomly from 1 to 5 seconds.
        .set_wait_time(Duration::from_secs(1), Duration::from_secs(5))?
        .register_transaction(test_start)
        .register_transaction(update_small_model)
        .register_transaction(get_small_model)
        .register_transaction(update_large_model)
        .register_transaction(get_large_model))
}

#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(user: &mut GooseUser, cli: CeramicHttpClient) -> TransactionResult {
    let small_model = ModelDefinition::new::<models::SmallModel>(
        "load_test_small_model",
        ModelAccountRelation::List,
    )
    .unwrap();
    let small_model_id = setup_model(user, &cli, small_model).await?;
    let small_model_instance_id =
        setup_model_instance(user, &cli, &small_model_id, &models::SmallModel::random()).await?;
    let large_model = ModelDefinition::new::<models::LargeModel>(
        "load_test_large_model",
        ModelAccountRelation::List,
    )
    .unwrap();
    let large_model_id = setup_model(user, &cli, large_model).await?;
    let large_model_instance_id =
        setup_model_instance(user, &cli, &large_model_id, &models::LargeModel::random()).await?;

    let user_data = LoadTestUserData {
        cli,
        small_model_id,
        small_model_instance_id,
        large_model_id,
        large_model_instance_id,
    };
    debug!(?user_data, "user data");

    user.set_session_data(user_data);

    Ok(())
}

async fn update_small_model(user: &mut GooseUser) -> TransactionResult {
    let (model, url, req) = {
        let user_data: &LoadTestUserData = user.get_session_data_unchecked();
        let model = user_data.small_model_id.clone();
        let cli = &user_data.cli;
        let streams_url = user.build_url(&format!(
            "{}/{}",
            cli.streams_endpoint(),
            user_data.small_model_instance_id
        ))?;
        let req = GooseRequest::builder()
            .method(GooseMethod::Get)
            .set_request_builder(user.client.get(streams_url))
            .expect_status_code(200)
            .build();
        let commits_url = user.build_url(cli.commits_endpoint())?;
        (model, commits_url, req)
    };
    let resp = user.request(req).await?;
    let resp: StreamsResponseOrError = resp.response?.json().await?;
    let resp = resp.resolve("update_small_model_get").unwrap();

    let req = {
        let user_data: &LoadTestUserData = user.get_session_data_unchecked();
        user_data
            .cli
            .create_replace_request(&model, &resp, &models::SmallModel::random())
            .await
            .unwrap()
    };
    let req = user.client.post(url).json(&req);
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(req)
        .expect_status_code(200)
        .build();
    let resp = user.request(req).await?;
    let resp: StreamsResponseOrError = resp.response?.json().await?;
    resp.resolve("update_small_model").unwrap();
    Ok(())
}

async fn get_small_model(user: &mut GooseUser) -> TransactionResult {
    let user_data: &LoadTestUserData = user.get_session_data_unchecked();
    let cli: &CeramicHttpClient = &user_data.cli;
    let url = user.build_url(&format!(
        "{}/{}",
        cli.streams_endpoint(),
        user_data.small_model_instance_id
    ))?;
    let resp = user.get(&url).await?;
    let resp: StreamsResponseOrError = resp.response?.json().await?;
    resp.resolve("get_small_model").unwrap();
    Ok(())
}

async fn update_large_model(user: &mut GooseUser) -> TransactionResult {
    let (model, url, req) = {
        let user_data: &LoadTestUserData = user.get_session_data_unchecked();
        let model = user_data.large_model_id.clone();
        let cli = &user_data.cli;
        let streams_url = user.build_url(&format!(
            "{}/{}",
            cli.streams_endpoint(),
            user_data.large_model_instance_id
        ))?;
        let req = GooseRequest::builder()
            .method(GooseMethod::Get)
            .set_request_builder(user.client.get(streams_url))
            .expect_status_code(200)
            .build();
        let commits_url = user.build_url(cli.commits_endpoint())?;
        (model, commits_url, req)
    };
    let resp = user.request(req).await?;
    let resp: StreamsResponseOrError = resp.response?.json().await?;
    let resp = resp.resolve("update_large_model_get").unwrap();

    let req = {
        let user_data: &LoadTestUserData = user.get_session_data_unchecked();
        user_data
            .cli
            .create_replace_request(&model, &resp, &models::LargeModel::random())
            .await
            .unwrap()
    };
    let req = user.client.post(url).json(&req);
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(req)
        .expect_status_code(200)
        .build();
    let result = user.request(req).await?;
    let resp: StreamsResponseOrError = result.response?.json().await?;
    resp.resolve("update_large_model").unwrap();
    Ok(())
}

async fn get_large_model(user: &mut GooseUser) -> TransactionResult {
    let user_data: &LoadTestUserData = user.get_session_data_unchecked();
    let cli: &CeramicHttpClient = &user_data.cli;
    let url = user.build_url(&format!(
        "{}/{}",
        cli.streams_endpoint(),
        user_data.large_model_instance_id
    ))?;
    let resp = user.get(&url).await?;
    let resp: StreamsResponseOrError = resp.response?.json().await?;
    resp.resolve("get_large_model").unwrap();
    Ok(())
}

pub async fn setup_model(
    user: &mut GooseUser,
    cli: &CeramicHttpClient,
    model: ModelDefinition,
) -> Result<StreamId, TransactionError> {
    let url = user.build_url(cli.streams_endpoint())?;
    let req = cli.create_model_request(&model).await.unwrap();
    let req = user.client.post(url).json(&req);
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(req)
        .expect_status_code(200)
        .build();
    let result = user.request(req).await?;
    let resp: StreamsResponseOrError = result.response?.json().await?;
    Ok(resp.resolve("setup_model").unwrap().stream_id)
}

pub async fn setup_model_instance<T: Serialize>(
    user: &mut GooseUser,
    cli: &CeramicHttpClient,
    model: &StreamId,
    data: &T,
) -> Result<StreamId, TransactionError> {
    let url = user.build_url(cli.streams_endpoint())?;
    let req = cli.create_list_instance_request(model, data).await.unwrap();
    let req = user.client.post(url).json(&req);
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(req)
        .expect_status_code(200)
        .build();
    let result = user.request(req).await?;
    let resp: StreamsResponseOrError = result.response?.json().await?;
    Ok(resp.resolve("setup_model").unwrap().stream_id)
}
