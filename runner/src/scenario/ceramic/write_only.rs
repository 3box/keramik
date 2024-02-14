use goose::prelude::*;
use std::sync::Arc;

use crate::scenario::ceramic::simple::{setup, update_large_model, update_small_model};

use super::{model_instance::CeramicModelInstanceTestUser, CeramicScenarioParameters};

pub async fn scenario(params: CeramicScenarioParameters) -> Result<Scenario, GooseError> {
    let config = CeramicModelInstanceTestUser::prep_scenario(params)
        .await
        .unwrap();
    let setup = Transaction::new(Arc::new(move |user| Box::pin(setup(user, config.clone()))))
        .set_name("setup")
        .set_on_start();

    let update_small_model = transaction!(update_small_model).set_name("update_small_model");

    let update_large_model = transaction!(update_large_model).set_name("update_large_model");

    Ok(scenario!("CeramicWriteOnly")
        .register_transaction(setup)
        .register_transaction(update_small_model)
        .register_transaction(update_large_model))
}
