use crate::scenario::ceramic::model_reuse::{
    get_model_id, set_model_id, ModelReuseLoadTestUserData,
};
use crate::scenario::ceramic::models;
use crate::scenario::ceramic::util::setup_model;
use crate::scenario::get_redis_client;
use ceramic_core::{Cid, EventId};
use ceramic_http_client::{CeramicHttpClient, ModelAccountRelation, ModelDefinition};
use goose::prelude::*;
use libipld::cid;
use multihash::{Code, MultihashDigest};
use reqwest::Url;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::{sync::Arc, time::Duration};
use tracing::{info, instrument};

use super::util::goose_error;
use super::{CeramicClient, Credentials};

const MODEL_ID_KEY: &str = "event_id_sync_model_id";
pub(crate) const CREATE_EVENT_TX_NAME: &str = "create_new_event";
// goose stores the HTTP method + transaction name as the request name
// it's a lot simpler to access request metrics (a map) than tx metrics (a vec<vec>)
pub(crate) const CREATE_EVENT_REQ_NAME: &str = "POST create_new_event";

static FIRST_USER: AtomicBool = AtomicBool::new(true);
static NEW_EVENT_CNT: AtomicU64 = AtomicU64::new(0);

fn should_request_events() -> bool {
    goose::get_worker_id() == 1
}

/// we only want one user to create and subscribe to the model
fn is_first_user() -> bool {
    FIRST_USER.swap(false, std::sync::atomic::Ordering::SeqCst)
}

// accept option as goose manager builds the scenario as well, but doesn't need any peers and won't run it so it will always be Some in execution
pub async fn event_id_sync_scenario(
    ipfs_peer_addr: Option<String>,
) -> Result<Scenario, GooseError> {
    let ipfs_addr: Url = ipfs_peer_addr
        .map(|u| u.parse().unwrap())
        .expect("missing ipfs peer address in event ID scenario");
    let creds = Credentials::from_env().await.map_err(goose_error)?;
    let cli = CeramicHttpClient::new(creds.signer);
    let redis_cli = get_redis_client().await?;

    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(
            user,
            cli.clone(),
            redis_cli.clone(),
            ipfs_addr.clone(),
        ))
    }))
    .set_name("setup")
    .set_on_start();

    let create_new_event = transaction!(create_new_event).set_name(CREATE_EVENT_TX_NAME);

    Ok(scenario!("EventIDSync")
        .register_transaction(test_start)
        .register_transaction(create_new_event))
}

/// One user on one node creates a model.
/// One user on each node subscribes to the model via Recon
#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(
    user: &mut GooseUser,
    cli: CeramicClient,
    redis_cli: redis::Client,
    ipfs_peer_addr: Url,
) -> TransactionResult {
    let mut conn = redis_cli.get_async_connection().await.unwrap();
    let model_id = if should_request_events() && is_first_user() {
        info!("creating model for event ID sync test");
        let small_model = match ModelDefinition::new::<models::SmallModel>(
            "load_test_small_model",
            ModelAccountRelation::List,
        ) {
            Ok(model) => model,
            Err(e) => {
                tracing::error!("failed to create model: {}", e);
                panic!("failed to create model: {}", e);
            }
        };
        let model_id = match setup_model(user, &cli, small_model).await {
            Ok(model_id) => model_id,
            Err(e) => {
                tracing::error!("failed to setup model: {:?}", e);
                return Err(e);
            }
        };
        set_model_id(&mut conn, &model_id, MODEL_ID_KEY).await;
        model_id
    } else {
        get_model_id(&mut conn, MODEL_ID_KEY).await
    };

    tracing::debug!(%model_id, "syncing model");

    let path = format!("/ceramic/subscribe/model/{}?limit=1", model_id);
    let user_data = ModelReuseLoadTestUserData {
        cli,
        redis_cli,
        model_id,
    };
    user.set_session_data(user_data);
    user.base_url = Some(ipfs_peer_addr); // Recon is only available on IPFS address right now

    let request_builder = user
        .get_request_builder(&GooseMethod::Get, &path)?
        .timeout(Duration::from_secs(5));
    let req = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(200)
        .build();

    let _goose = user.request(req).await?;

    Ok(())
}

/// Generate a random event that the nodes are interested in. Only one node should create but all
/// users do it so that we can generate a lot of events.
async fn create_new_event(user: &mut GooseUser) -> TransactionResult {
    if !should_request_events() {
        // No work is performed while awaiting on the sleep future to complete (from tokio::time::sleep docs)
        // it's not high resolution but we don't need it to be since we're already waiting half a second
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    } else {
        let user_data: &ModelReuseLoadTestUserData = user
            .get_session_data()
            .expect("we are missing sync_event_id user data");

        // eventId needs to be a multibase encoded string for the API to accept it
        let event_id = format!("F{}", random_event_id(&user_data.model_id.to_string()));
        let event_key_body = serde_json::json!({"eventId": event_id});
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
