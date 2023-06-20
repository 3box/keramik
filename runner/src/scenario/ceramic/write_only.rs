use ceramic_http_client::CeramicHttpClient;
use goose::prelude::*;
use std::{sync::Arc, time::Duration};

use crate::scenario::ceramic::{setup, update_large_model, update_small_model, Credentials};

pub fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::new();
    let cli = CeramicHttpClient::new(creds.signer, &creds.private_key);

    let setup_cli = cli;
    let setup = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, setup_cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();

    let update_small_model = transaction!(update_small_model).set_name("update_small_model");

    let update_large_model = transaction!(update_large_model).set_name("update_large_model");

    Ok(scenario!("CeramicWriteOnly")
        .set_wait_time(Duration::from_millis(10), Duration::from_millis(100))?
        .register_transaction(setup)
        .register_transaction(update_small_model)
        .register_transaction(update_large_model))
}
