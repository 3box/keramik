use std::str::FromStr;
use std::{sync::Arc, time::Duration};

use ceramic_http_client::ceramic_event::StreamId;
use goose::prelude::*;
use redis::AsyncCommands;
use tracing::instrument;

use crate::scenario::ceramic::model_instance::{
    CeramicModelInstanceTestUser, ModelInstanceRequests,
};
use crate::scenario::ceramic::models::LargeModel;
use crate::scenario::ceramic::CeramicScenarioParameters;

use super::model_instance::EnvBasedConfig;

const MODEL_INSTANCE_ID_KEY: &str = "model_reuse_model_instance_id";

pub async fn scenario(params: CeramicScenarioParameters) -> Result<Scenario, GooseError> {
    let config = CeramicModelInstanceTestUser::prep_scenario(params)
        .await
        .unwrap();
    let test_start = Transaction::new(Arc::new(move |user| Box::pin(setup(user, config.clone()))))
        .set_name("setup")
        .set_on_start();

    let create_instance_tx = transaction!(create_instance).set_name("create_instance");
    let get_instance_tx = transaction!(get_instance).set_name("get_instance");

    Ok(scenario!("CeramicModelReuseScenario")
        .register_transaction(test_start)
        .register_transaction(create_instance_tx)
        .register_transaction(get_instance_tx))
}

#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(user: &mut GooseUser, config: EnvBasedConfig) -> TransactionResult {
    CeramicModelInstanceTestUser::setup_scenario(user, config)
        .await
        .unwrap(); //panic??

    Ok(())
}

async fn create_instance(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let cli = &user_data.user_cli();
    let mut conn = user_data.redis_cli().get_async_connection().await.unwrap();

    let id = ModelInstanceRequests::create_model_instance(
        user,
        cli,
        &user_data.large_model_id,
        "model_reuse",
        &LargeModel {
            creator: "keramik".to_string(),
            name: "model-reuse-model-instance".to_string(),
            description: "a".to_string(),
            tpe: 10,
        },
    )
    .await?;

    let _: () = conn
        .rpush(MODEL_INSTANCE_ID_KEY, id.to_string())
        .await
        .unwrap();

    Ok(())
}

async fn get_model_instance_id(conn: &mut redis::aio::Connection) -> StreamId {
    loop {
        let len: usize = conn.llen(MODEL_INSTANCE_ID_KEY).await.unwrap();
        if len > 0 {
            let id: String = conn.lpop(MODEL_INSTANCE_ID_KEY, None).await.unwrap();
            return StreamId::from_str(&id).unwrap();
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

async fn get_instance(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let mut redis_conn = user_data.redis_cli().get_async_connection().await.unwrap();
    let model_instance_id = get_model_instance_id(&mut redis_conn).await;

    ModelInstanceRequests::get_stream_tx(
        user,
        user_data.user_cli(),
        &model_instance_id,
        "model_reuse_get_instance",
    )
    .await
}
