use crate::scenario::ceramic::model_instance::{loop_until_key_value_set, set_key_to_stream_id};
use crate::scenario::{
    get_redis_client, is_first_goose_user, is_goose_leader, reset_first_goose_user,
};
use ceramic_core::{Cid, EventId};
use ceramic_http_client::ceramic_event::{StreamId, StreamIdType};
use goose::prelude::*;
use libipld::cid;
use multihash::{Code, MultihashDigest};
use rand::rngs::ThreadRng;
use rand::Rng;
use std::sync::atomic::AtomicU64;
use std::{sync::Arc, time::Duration};
use tracing::{info, instrument};

const MODEL_ID_KEY: &str = "event_id_sync_model_id";
pub(crate) const CREATE_EVENT_TX_NAME: &str = "create_new_event";
// goose stores the HTTP method + transaction name as the request name
// it's a lot simpler to access request metrics (a map) than tx metrics (a vec<vec>)
pub(crate) const CREATE_EVENT_REQ_NAME: &str = "POST create_new_event";

static NEW_EVENT_CNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES_GENERATED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
struct ReconCeramicModelInstanceTestUser {
    model_id: StreamId,
    with_data: bool,
}

async fn init_scenario(with_data: bool) -> Result<Transaction, GooseError> {
    let redis_cli = get_redis_client().await?;

    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, redis_cli.clone(), with_data))
    }))
    .set_name("setup")
    .set_on_start();
    Ok(test_start)
}

async fn log_results(_user: &mut GooseUser) -> TransactionResult {
    if is_first_goose_user() {
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
    let test_start = init_scenario(true).await?;
    let create_new_event = transaction!(create_new_event).set_name(CREATE_EVENT_TX_NAME);
    let stop = transaction!(log_results)
        .set_name("log_results")
        .set_on_stop();
    Ok(scenario!("ReconSync")
        .register_transaction(test_start)
        .register_transaction(create_new_event)
        .register_transaction(stop))
}

// accept option as goose manager builds the scenario as well, but doesn't need any peers and won't run it so it will always be Some in execution
pub async fn event_key_sync_scenario() -> Result<Scenario, GooseError> {
    let test_start = init_scenario(false).await?;

    let create_new_event = transaction!(create_new_event).set_name(CREATE_EVENT_TX_NAME);
    let reset_single_user = transaction!(reset_first_user)
        .set_name("reset_first_user")
        .set_on_start();

    Ok(scenario!("ReconKeySync")
        .register_transaction(test_start)
        .register_transaction(reset_single_user)
        .register_transaction(create_new_event))
}

async fn reset_first_user(_user: &mut GooseUser) -> TransactionResult {
    reset_first_goose_user();
    Ok(())
}

/// One user on one node creates a model.
/// One user on each node subscribes to the model via Recon
#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(
    user: &mut GooseUser,
    redis_cli: redis::Client,
    with_data: bool,
) -> TransactionResult {
    let mut conn = redis_cli.get_async_connection().await.unwrap();
    let first = is_first_goose_user();
    let model_id = if is_goose_leader() && first {
        info!("creating model for event ID sync test");
        // We only need a model ID we do not need it to be a real model.
        let model_id = StreamId {
            r#type: StreamIdType::Model,
            cid: random_cid(),
        };
        set_key_to_stream_id(&mut conn, MODEL_ID_KEY, &model_id).await;
        model_id
    } else {
        loop_until_key_value_set(&mut conn, MODEL_ID_KEY).await
    };

    tracing::debug!(%model_id, "syncing model");

    let path = format!("/ceramic/interests/model/{}", model_id);
    let user_data = ReconLoadTestUserData {
        model_id,
        with_data,
    };
    user.set_session_data(user_data);

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
    if !is_goose_leader() {
        // No work is performed while awaiting on the sleep future to complete (from tokio::time::sleep docs)
        // it's not high resolution but we don't need it to be since we're already waiting half a second
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    } else {
        let user_data: &ReconCeramicModelInstanceTestUser = user
            .get_session_data()
            .expect("we are missing sync_event_id user data");

        // eventId needs to be a multibase encoded string for the API to accept it
        let event_id = format!("F{}", random_event_id(&user_data.model_id.to_string()));
        let event_key_body = if user_data.with_data {
            let payload = random_car_1kb_body().await;
            serde_json::json!({"id": event_id, "data": payload})
        } else {
            serde_json::json!({"id": event_id})
        };
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

fn random_cid() -> cid::Cid {
    let mut data = [0u8; 8];
    rand::Rng::fill(&mut rand::thread_rng(), &mut data);
    let hash = Code::Sha2_256.digest(data.as_slice());
    Cid::new_v1(0x00, hash)
}

const SORT_KEY: &str = "model";
// hard code test controller in case we want to find/prune later
const TEST_CONTROLLER: &str = "did:key:z6MkoFUppcKEVYTS8oVidrja94UoJTatNhnhxJRKF7NYPScS";

fn random_event_id(sort_value: &str) -> ceramic_core::EventId {
    let cid = random_cid();
    EventId::new(
        &ceramic_core::Network::Local(0),
        SORT_KEY,
        sort_value,
        TEST_CONTROLLER,
        &cid,
        0,
        &cid,
    )
}

fn random_block() -> (Cid, Vec<u8>) {
    let mut rng = rand::thread_rng();
    TOTAL_BYTES_GENERATED.fetch_add(1000, std::sync::atomic::Ordering::Relaxed);
    let unique: [u8; 1000] = gen_rand_bytes(&mut rng);

    let hash = ::multihash::MultihashDigest::digest(&::multihash::Code::Sha2_256, &unique);
    (Cid::new_v1(0x00, hash), unique.to_vec())
}

async fn random_car_1kb_body() -> String {
    let mut bytes = Vec::with_capacity(1500);
    let (cid, block) = random_block();
    let roots = vec![cid];
    let mut writer = iroh_car::CarWriter::new(iroh_car::CarHeader::V1(roots.into()), &mut bytes);
    writer.write(cid, block).await.unwrap();
    writer.finish().await.unwrap();

    multibase::encode(multibase::Base::Base36Lower, bytes)
}

fn gen_rand_bytes<const SIZE: usize>(rng: &mut ThreadRng) -> [u8; SIZE] {
    let mut arr = [0; SIZE];
    for x in &mut arr {
        *x = rng.gen_range(0..=255);
    }
    arr
}
