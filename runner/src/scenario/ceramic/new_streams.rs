use std::sync::Arc;

use goose::prelude::*;

use crate::scenario::ceramic::{models, simple::setup, RandomModelInstance};

use super::{
    model_instance::{CeramicModelInstanceTestUser, ModelInstanceRequests},
    CeramicScenarioParameters,
};

pub async fn scenario(params: CeramicScenarioParameters) -> Result<Scenario, GooseError> {
    let config = CeramicModelInstanceTestUser::prep_scenario(params)
        .await
        .unwrap();

    let test_start = Transaction::new(Arc::new(move |user| Box::pin(setup(user, config.clone()))))
        .set_name("setup")
        .set_on_start();

    let instantiate_small_model =
        transaction!(instantiate_small_model).set_name("instantiate_small_model");
    let instantiate_large_model =
        transaction!(instantiate_large_model).set_name("instantiate_large_model");

    Ok(scenario!("CeramicNewStreams")
        .register_transaction(test_start)
        .register_transaction(instantiate_small_model)
        .register_transaction(instantiate_large_model))
}

async fn instantiate_small_model(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    ModelInstanceRequests::create_model_instance(
        user,
        user_data.user_cli(),
        &user_data.small_model_id,
        "instantiate_small_model_instance",
        &models::SmallModel::random(),
    )
    .await?;
    Ok(())
}

async fn instantiate_large_model(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    ModelInstanceRequests::create_model_instance(
        user,
        user_data.user_cli(),
        &user_data.large_model_id,
        "instantiate_large_model_instance",
        &models::LargeModel::random(),
    )
    .await?;
    Ok(())
}
