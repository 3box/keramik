use super::{models::RandomModelInstance, CeramicClient, Credentials};

use crate::goose_try;
use crate::scenario::ceramic::models;
use crate::scenario::ceramic::util::{goose_error, setup_model, setup_model_instance};

use ceramic_http_client::api::StreamsResponseOrError;
use ceramic_http_client::ceramic_event::StreamId;
use ceramic_http_client::CeramicHttpClient;
use ceramic_http_client::{ModelAccountRelation, ModelDefinition};
use goose::prelude::*;
use std::sync::Arc;
use tracing::instrument;

pub(crate) struct LoadTestUserData {
    pub cli: CeramicClient,
    pub small_model_id: StreamId,
    pub small_model_instance_id: StreamId,
    pub large_model_id: StreamId,
    pub large_model_instance_id: StreamId,
}

#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
pub(crate) async fn setup(user: &mut GooseUser, cli: CeramicClient) -> TransactionResult {
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

    user.set_session_data(user_data);

    Ok(())
}

pub async fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::from_env().await.map_err(goose_error)?;
    let cli = CeramicHttpClient::new(creds.signer);

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
        .register_transaction(test_start)
        .register_transaction(update_small_model)
        .register_transaction(get_small_model)
        .register_transaction(update_large_model)
        .register_transaction(get_large_model))
}

pub(crate) async fn update_small_model(user: &mut GooseUser) -> TransactionResult {
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
    let mut goose = user
        .request(
            GooseRequest::builder()
                .method(GooseMethod::Post)
                .set_request_builder(req)
                .expect_status_code(200)
                .build(),
        )
        .await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "update",
        &mut goose.request,
        resp.resolve("update_small_model")
    )?;
    Ok(())
}

pub(crate) async fn get_small_model(user: &mut GooseUser) -> TransactionResult {
    let user_data: &LoadTestUserData = user.get_session_data_unchecked();
    let cli: &CeramicClient = &user_data.cli;
    let url = user.build_url(&format!(
        "{}/{}",
        cli.streams_endpoint(),
        user_data.small_model_instance_id
    ))?;
    let mut goose = user.get(&url).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "get",
        &mut goose.request,
        resp.resolve("get_small_instance")
    )?;
    Ok(())
}

pub(crate) async fn update_large_model(user: &mut GooseUser) -> TransactionResult {
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
    let mut goose = user.request(req).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    let resp = goose_try!(user, "update", &mut goose.request, {
        resp.resolve("update_large_model_get")
    })?;

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
    let mut goose = user.request(req).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "update",
        &mut goose.request,
        resp.resolve("update_large_model")
    )?;
    Ok(())
}

pub(crate) async fn get_large_model(user: &mut GooseUser) -> TransactionResult {
    let user_data: &LoadTestUserData = user.get_session_data_unchecked();
    let cli: &CeramicClient = &user_data.cli;
    let url = user.build_url(&format!(
        "{}/{}",
        cli.streams_endpoint(),
        user_data.large_model_instance_id
    ))?;
    let mut goose = user.get(&url).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "get",
        &mut goose.request,
        resp.resolve("get_large_instance")
    )?;
    Ok(())
}
