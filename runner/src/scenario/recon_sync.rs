use std::str::FromStr;
use std::sync::atomic::AtomicU64;
use std::{sync::Arc, time::Duration};

use ceramic_http_client::ceramic_event::StreamId;
use goose::prelude::*;
use tracing::{info, instrument};

use crate::scenario::ceramic::model_instance::{loop_until_key_value_set, set_key_to_stream_id};
use crate::scenario::{
    get_redis_client, is_goose_global_leader, is_goose_lead_user, is_goose_lead_worker,
};

use super::util::random_init_event_car;

const MODEL_ID_KEY: &str = "event_id_sync_model_id";
pub(crate) const CREATE_EVENT_TX_NAME: &str = "create_new_event";
// goose stores the HTTP method + transaction name as the request name
// it's a lot simpler to access request metrics (a map) than tx metrics (a vec<vec>)
pub(crate) const CREATE_EVENT_REQ_NAME: &str = "POST create_new_event";

static NEW_EVENT_CNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_GENERATED: AtomicU64 = AtomicU64::new(0);

async fn init_scenario() -> Result<Transaction, GooseError> {
    let redis_cli = get_redis_client().await?;

    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, redis_cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();
    Ok(test_start)
}

async fn log_results(_user: &mut GooseUser) -> TransactionResult {
    if is_goose_lead_user() {
        let cnt = NEW_EVENT_CNT.load(std::sync::atomic::Ordering::Relaxed);
        let bytes = TOTAL_BYTES_GENERATED.load(std::sync::atomic::Ordering::Relaxed);
        info!(
            "created {} events with {} bytes ({} MB) of data",
            cnt,
            bytes,
            bytes / 1_000_000 // or do we want to measure with 1024...
        );
    }
    Ok(())
}

pub async fn event_sync_scenario() -> Result<Scenario, GooseError> {
    let test_start = init_scenario().await?;
    let create_new_event = transaction!(create_new_event).set_name(CREATE_EVENT_TX_NAME);
    let stop = transaction!(log_results)
        .set_name("log_results")
        .set_on_stop();
    Ok(scenario!("ReconSync")
        .register_transaction(test_start)
        .register_transaction(create_new_event)
        .register_transaction(stop))
}

/// One user on one node creates a model.
/// One user on each node subscribes to the model via Recon
#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(user: &mut GooseUser, redis_cli: redis::Client) -> TransactionResult {
    let mut conn = redis_cli.get_async_connection().await.unwrap();
    let first = is_goose_global_leader(is_goose_lead_user());

    let model_id = if first {
        info!("creating model for event ID sync test");
        // We only need a model ID we do not need it to be a real model.
        // CID version mismatch between c1 versions so we just hard code one
        let model_id =
            StreamId::from_str("kjzl6kcym7w8y7nzgytqayf6aro12zt0mm01n6ydjomyvvklcspx9kr6gpbwd09")
                .unwrap();
        set_key_to_stream_id(&mut conn, MODEL_ID_KEY, &model_id).await;

        // TODO: set a real model

        model_id
    } else {
        loop_until_key_value_set(&mut conn, MODEL_ID_KEY).await
    };

    tracing::debug!(%model_id, "syncing model");

    let path = format!("/ceramic/interests/model/{}", model_id);

    let request_builder = user
        .get_request_builder(&GooseMethod::Post, &path)?
        .timeout(Duration::from_secs(5));
    let req = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(204)
        .build();

    let _goose = user.request(req).await?;
    Ok(())
}

/// Generate a random event that the nodes are interested in. Only one node should create but all
/// users do it so that we can generate a lot of events.
async fn create_new_event(user: &mut GooseUser) -> TransactionResult {
    if !is_goose_lead_worker() {
        // No work is performed while awaiting on the sleep future to complete (from tokio::time::sleep docs)
        // it's not high resolution but we don't need it to be since we're already waiting half a second
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    } else {
        let model_id =
            StreamId::from_str("kjzl6kcym7w8y7nzgytqayf6aro12zt0mm01n6ydjomyvvklcspx9kr6gpbwd09")
                .unwrap();
        let data = random_init_event_car(
            SORT_KEY,
            model_id.to_vec().unwrap(),
            Some(TEST_CONTROLLER.to_string()),
        )
        .await
        .unwrap();
        let event_key_body = serde_json::json!({"data": data});
        // eventId needs to be a multibase encoded string for the API to accept it

        let cnt = NEW_EVENT_CNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if cnt == 0 || cnt % 1000 == 0 {
            tracing::trace!("new sync_event_id body: {:?}", event_key_body);
        }

        let request_builder = user
            .get_request_builder(&GooseMethod::Post, "/ceramic/events")?
            .timeout(Duration::from_secs(1))
            .json(&event_key_body);
        let req = GooseRequest::builder()
            .method(GooseMethod::Post)
            .set_request_builder(request_builder)
            .expect_status_code(204)
            .build();
        let _resp = user.request(req).await?;
        Ok(())
    }
}

const SORT_KEY: &str = "model";
// hard code test controller in case we want to find/prune later
const TEST_CONTROLLER: &str = "did:key:z6MkoFUppcKEVYTS8oVidrja94UoJTatNhnhxJRKF7NYPScS";
