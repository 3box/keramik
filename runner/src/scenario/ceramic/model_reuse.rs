use crate::goose_try;
use crate::scenario::ceramic::models::LargeModel;
use crate::scenario::ceramic::util::{goose_error, index_model, setup_model, setup_model_instance};
use crate::scenario::ceramic::{CeramicClient, Credentials};
use crate::scenario::get_redis_client;
use ceramic_http_client::api::StreamsResponseOrError;
use ceramic_http_client::ceramic_event::{JwkSigner, StreamId};
use ceramic_http_client::{CeramicHttpClient, ModelAccountRelation, ModelDefinition};
use goose::prelude::*;
use redis::AsyncCommands;
use std::str::FromStr;
use std::{sync::Arc, time::Duration};
use tracing::instrument;

#[derive(Clone)]
struct ModelReuseLoadTestUserData {
    cli: CeramicHttpClient<JwkSigner>,
    redis_cli: redis::Client,
    model_id: StreamId,
}

const MODEL_ID_KEY: &str = "model_reuse_model_id";
const MODEL_INSTANCE_ID_KEY: &str = "model_reuse_model_instance_id";

pub async fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::from_env().await.map_err(goose_error)?;
    let cli = CeramicHttpClient::new(creds.signer);
    let redis_cli = get_redis_client().await?;

    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, cli.clone(), redis_cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();

    let create_instance_tx = transaction!(create_instance).set_name("create_instance");
    let get_instance_tx = transaction!(get_instance).set_name("get_instance");

    Ok(scenario!("CeramicModelReuseScenario")
        // After each transactions runs, sleep randomly from 1 to 5 seconds.
        .set_wait_time(Duration::from_secs(1), Duration::from_secs(5))?
        .register_transaction(test_start)
        .register_transaction(create_instance_tx)
        .register_transaction(get_instance_tx))
}

async fn get_model_id(conn: &mut redis::aio::Connection) -> StreamId {
    loop {
        if conn.exists(MODEL_ID_KEY).await.unwrap() {
            let id: String = conn.get(MODEL_ID_KEY).await.unwrap();
            return StreamId::from_str(&id).unwrap();
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(
    user: &mut GooseUser,
    cli: CeramicClient,
    redis_cli: redis::Client,
) -> TransactionResult {
    let mut conn = redis_cli.get_async_connection().await.unwrap();
    let model_id = if user.weighted_users_index == 0 {
        let model_definition = ModelDefinition::new::<LargeModel>(
            "model_reuse_query_model",
            ModelAccountRelation::List,
        )
        .unwrap();
        let model_id = setup_model(user, &cli, model_definition).await?;
        index_model(user, &cli, &model_id).await?;

        let _: () = conn.set(MODEL_ID_KEY, model_id.to_string()).await.unwrap();

        model_id
    } else {
        get_model_id(&mut conn).await
    };

    let user_data = ModelReuseLoadTestUserData {
        cli,
        redis_cli,
        model_id,
    };

    user.set_session_data(user_data);

    Ok(())
}

async fn create_instance(user: &mut GooseUser) -> TransactionResult {
    let user_data: ModelReuseLoadTestUserData = {
        let data: &ModelReuseLoadTestUserData = user.get_session_data_unchecked();
        data.clone()
    };
    let cli = &user_data.cli;
    let mut conn = user_data.redis_cli.get_async_connection().await.unwrap();

    let id = setup_model_instance(
        user,
        cli,
        &user_data.model_id,
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
    let user_data: &ModelReuseLoadTestUserData = user.get_session_data_unchecked();
    let cli: &CeramicClient = &user_data.cli;
    let mut redis_conn = user_data.redis_cli.get_async_connection().await.unwrap();
    let model_instance_id = get_model_instance_id(&mut redis_conn).await;
    let url = user.build_url(&format!("{}/{}", cli.streams_endpoint(), model_instance_id,))?;
    let mut goose = user.get(&url).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "get",
        &mut goose.request,
        resp.resolve("get_instance")
    )?;
    Ok(())
}
