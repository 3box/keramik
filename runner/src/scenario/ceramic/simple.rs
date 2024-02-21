use std::sync::Arc;

use crate::scenario::ceramic::{
    model_instance::CeramicModelInstanceTestUser,
    models::{self, RandomModelInstance},
};
use goose::prelude::*;

use super::{model_instance::ModelInstanceRequests, CeramicScenarioParameters};

// unique_dids: if true, each user will create a new DID otherwise will share one admin DID
pub async fn scenario(params: CeramicScenarioParameters) -> Result<Scenario, GooseError> {
    let config = CeramicModelInstanceTestUser::prep_scenario(params)
        .await
        .unwrap();
    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(CeramicModelInstanceTestUser::setup_mid_scenario(
            user,
            config.clone(),
        ))
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
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let data = models::SmallModel::random();
    ModelInstanceRequests::get_and_replace_stream_tx(
        user,
        user_data.user_cli(),
        &user_data.small_model_id,
        user_data.small_model_instance_ids.first().unwrap(),
        "update_small_model_instance",
        &data,
    )
    .await
}

pub(crate) async fn get_small_model(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    ModelInstanceRequests::get_stream_tx(
        user,
        user_data.user_cli(),
        user_data.large_model_instance_ids.first().unwrap(),
        "small_model_instance",
    )
    .await
}

pub(crate) async fn update_large_model(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let data = models::LargeModel::random();
    ModelInstanceRequests::get_and_replace_stream_tx(
        user,
        user_data.user_cli(),
        &user_data.large_model_id,
        user_data.large_model_instance_ids.first().unwrap(),
        "update_large_model_instance",
        &data,
    )
    .await
}

pub(crate) async fn get_large_model(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    ModelInstanceRequests::get_stream_tx(
        user,
        user_data.user_cli(),
        user_data.large_model_instance_ids.first().unwrap(),
        "large_model_instance",
    )
    .await
}
