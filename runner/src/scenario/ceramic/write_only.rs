use ceramic_http_client::CeramicHttpClient;
use goose::prelude::*;
use std::sync::Arc;

use crate::scenario::ceramic::simple::{setup, update_large_model, update_small_model};
use crate::scenario::ceramic::util::goose_error;

use super::Credentials;

pub async fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::from_env().await.map_err(goose_error)?;
    let cli = CeramicHttpClient::new(creds.signer);

    let setup_cli = cli;
    let setup = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, setup_cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();

    let update_small_model = transaction!(update_small_model).set_name("update_small_model");

    let update_large_model = transaction!(update_large_model).set_name("update_large_model");

    Ok(scenario!("CeramicWriteOnly")
        .register_transaction(setup)
        .register_transaction(update_small_model)
        .register_transaction(update_large_model))
}
