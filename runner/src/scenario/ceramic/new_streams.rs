use std::sync::Arc;

use goose::prelude::*;
use redis::{aio::MultiplexedConnection, AsyncCommands};
use tokio::sync::Mutex;
use tracing::info;

use crate::scenario::{
    ceramic::{model_instance::CountResponse, models, RandomModelInstance},
    get_redis_client,
};

use super::{
    model_instance::{CeramicModelInstanceTestUser, ModelInstanceRequests},
    CeramicScenarioParameters,
};

static REQWEST_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn default_pod_name(i: usize) -> String {
    format!("worker_{}", i)
}

/// returns (worker_id, count)
pub async fn benchmark_scenario_metrics(worker_cnt: usize, nonce: u64) -> Vec<(String, i32)> {
    let redis_cli = get_redis_client().await.unwrap();
    let mut conn = redis_cli.get_async_connection().await.unwrap();
    let mut res = vec![];
    // the worker IDs is 1 indexed, so we need to add 1 to the range
    for i in 1..worker_cnt + 1 {
        let before = conn
            .get::<_, Option<String>>(new_cnt_key(true, i, nonce))
            .await
            .unwrap();
        let after = conn
            .get::<_, Option<String>>(new_cnt_key(false, i, nonce))
            .await
            .unwrap();
        let name = conn
            .get::<_, Option<String>>(peer_name_key(i, nonce))
            .await
            .unwrap()
            .unwrap_or_else(|| default_pod_name(i));
        let before = before.and_then(|s| s.parse::<i32>().ok());
        let after = after.and_then(|s| s.parse::<i32>().ok());
        match (before, after) {
            (Some(before), Some(after)) => {
                res.push((name, after - before));
            }
            _ => {
                tracing::warn!("missing entry for worker {}", i);
            }
        }
    }
    res
}

fn new_cnt_key(before: bool, worker_id: usize, nonce: u64) -> String {
    let modifier = if before { "start" } else { "end" };
    format!("new_mid_count_{}_{}_{}", worker_id, modifier, nonce)
}

fn peer_name_key(worker_id: usize, nonce: u64) -> String {
    format!("peer_name_{}_{}", worker_id, nonce)
}

async fn store_metrics(user: &mut GooseUser, on_start: bool, nonce: u64) -> TransactionResult {
    // there is a goose bug where the manager closes the flume channel used to throttle requests, and then our on stop
    // requests all fail to execute. So instead of using the user to make the request, we use a reqwest client
    // if the throttle value is not set, it doesn't apply, but it's easier to just use the reqwest client in all cases
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    if user_data.user_info.lead_user {
        let url = user.build_url("/api/v0/collection/count")?;
        let client = REQWEST_CLIENT.get_or_init(reqwest::Client::new);
        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "model": &user_data.large_model_id.to_string(),
            }))
            .send()
            .await?
            .json::<CountResponse>()
            .await;
        let cnt = match resp {
            Ok(cnt) => cnt.count,
            Err(e) => {
                tracing::error!("failed to get model count: {}", e);
                if on_start {
                    0
                } else {
                    panic!("failed to get model count: {}", e);
                }
            }
        };
        info!(data=?cnt, stream_id=%user_data.large_model_id, "got model count");

        let peer_name = if let Some(url) = user.base_url.as_ref() {
            crate::simulate::parse_base_url_to_pod(url)
                .unwrap_or_else(|_| default_pod_name(goose::get_worker_id()))
        } else {
            tracing::warn!("failed to get base_url");
            default_pod_name(goose::get_worker_id())
        };

        let mut conn = user_data.redis_cli().get_async_connection().await.unwrap();
        let _: () = conn
            .set(
                new_cnt_key(on_start, goose::get_worker_id(), nonce),
                cnt.to_string(),
            )
            .await
            .unwrap();

        let _: () = conn
            .set(peer_name_key(goose::get_worker_id(), nonce), peer_name)
            .await
            .unwrap();
    }

    Ok(())
}

pub async fn small_large_scenario(
    params: CeramicScenarioParameters,
) -> Result<Scenario, GooseError> {
    let redis_cli = get_redis_client().await.unwrap();
    let multiplexed_conn = redis_cli.get_multiplexed_tokio_connection().await.unwrap();
    let shared_conn = Arc::new(Mutex::new(multiplexed_conn));
    
    let config = CeramicModelInstanceTestUser::prep_scenario(params.clone())
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

    let instantiate_small_model_conn = shared_conn.clone();
    let instantiate_small_model = Transaction::new(Arc::new(move |user| {
        let conn_clone = instantiate_small_model_conn.clone();
        Box::pin(async move {
            let mut conn = conn_clone.lock().await;
            instantiate_small_model(
                user,
                params.store_mids,
                &mut *conn
            ).await
        })
    }))
    .set_name("instantiate_small_model");

    let instantiate_large_model_conn = shared_conn.clone();
    let instantiate_large_model = Transaction::new(Arc::new(move |user| {
        let conn_clone = instantiate_large_model_conn.clone();
        Box::pin(async move {
            let mut conn = conn_clone.lock().await;
            instantiate_large_model(user, params.store_mids, &mut *conn).await
        })
    }))
    .set_name("instantiate_large_model");

    Ok(scenario!("CeramicNewStreams")
        .register_transaction(test_start)
        .register_transaction(instantiate_small_model)
        .register_transaction(instantiate_large_model))
}

// the nonce is used to ensure that the metrics stored in redis for the run are unique
pub async fn benchmark_scenario(
    params: CeramicScenarioParameters,
    nonce: u64,
) -> Result<Scenario, GooseError> {
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

    let before_metrics = Transaction::new(Arc::new(move |user| {
        Box::pin(store_metrics(user, true, nonce))
    }))
    .set_name("before_metrics")
    .set_on_start();

    let after_metrics = Transaction::new(Arc::new(move |user| {
        Box::pin(store_metrics(user, false, nonce))
    }))
    .set_name("after_metrics")
    .set_on_stop();

    let instantiate_large_model =
        transaction!(instantiate_large_model_1kb).set_name("instantiate_1k_mid");

    Ok(scenario!("CeramicNewStreamsBenchmark")
        .register_transaction(test_start)
        .register_transaction(before_metrics)
        .register_transaction(instantiate_large_model)
        .register_transaction(after_metrics))
}

async fn instantiate_small_model(user: &mut GooseUser, store_in_redis: bool, conn: &mut MultiplexedConnection) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let response = ModelInstanceRequests::create_model_instance(
        user,
        user_data.user_cli(),
        &user_data.small_model_id,
        "instantiate_small_model_instance",
        &models::SmallModel::random(),
    )
    .await?;
    if store_in_redis {
        let stream_id_string = response.to_string();
        let _: () = conn.sadd(format!("anchor_mids"), stream_id_string).await.unwrap();
    }
    Ok(())
}

async fn instantiate_large_model(user: &mut GooseUser, store_in_redis: bool, conn: &mut MultiplexedConnection) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let response = ModelInstanceRequests::create_model_instance(
        user,
        user_data.user_cli(),
        &user_data.large_model_id,
        "instantiate_large_model_instance",
        &models::LargeModel::random(),
    )
    .await?;
    if store_in_redis {
        let stream_id_string = response.to_string();
        let _: () = conn.sadd(format!("anchor_mids"), stream_id_string).await.unwrap();
    }
    Ok(())
}

async fn instantiate_large_model_1kb(user: &mut GooseUser) -> TransactionResult {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    if user_data.user_info.lead_worker {
        ModelInstanceRequests::create_model_instance(
            user,
            user_data.user_cli(),
            &user_data.large_model_id,
            "instantiate_large_model_instance",
            &models::LargeModel::random_1kb(),
        )
        .await?;
    } else {
        tracing::debug!(
            "Not lead worker. Just sleeping instead of creating large model instance document"
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // this doesn't block cpu
    }
    Ok(())
}
