use crate::goose_try;
use ceramic_http_client::CeramicHttpClient;
use goose::prelude::*;
use std::{sync::Arc, time::Duration};

use crate::scenario::ceramic::util::goose_error;
use crate::scenario::ceramic::{
    models, setup, Credentials, LoadTestUserData, RandomModelInstance, StreamsResponseOrError,
};

pub async fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::from_env().await.map_err(goose_error)?;
    let cli = CeramicHttpClient::new(creds.signer);

    let setup_cli = cli;
    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, setup_cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();

    let instantiate_small_model =
        transaction!(instantiate_small_model).set_name("instantiate_small_model");
    let instantiate_large_model =
        transaction!(instantiate_large_model).set_name("instantiate_large_model");

    Ok(scenario!("CeramicNewStreams")
        .set_wait_time(Duration::from_millis(10), Duration::from_millis(100))?
        .register_transaction(test_start)
        .register_transaction(instantiate_small_model)
        .register_transaction(instantiate_large_model))
}

async fn instantiate_small_model(user: &mut GooseUser) -> TransactionResult {
    let user_data: &LoadTestUserData = user.get_session_data_unchecked();
    let model = user_data.small_model_id.clone();
    let cli = &user_data.cli;
    let req = cli
        .create_list_instance_request(&model, &models::SmallModel::random())
        .await
        .unwrap();
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(
            user.client
                .post(user.build_url(cli.streams_endpoint())?)
                .json(&req),
        )
        .expect_status_code(200)
        .build();
    let mut goose = user.request(req).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "create_instance",
        &mut goose.request,
        resp.resolve("instantiate_small_model")
    )?;
    Ok(())
}

async fn instantiate_large_model(user: &mut GooseUser) -> TransactionResult {
    let user_data: &LoadTestUserData = user.get_session_data_unchecked();
    let model = user_data.large_model_id.clone();
    let cli = &user_data.cli;
    let url = user.build_url(cli.streams_endpoint())?;
    let req = cli
        .create_list_instance_request(&model, &models::LargeModel::random())
        .await
        .unwrap();
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
        "create_instance",
        &mut goose.request,
        resp.resolve("instantiate_large_model")
    )?;
    Ok(())
}
