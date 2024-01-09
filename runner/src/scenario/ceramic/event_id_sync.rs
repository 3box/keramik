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
use std::{sync::Arc, time::Duration};
use tracing::{info, instrument};

use super::util::goose_error;
use super::{CeramicClient, Credentials};

const MODEL_ID_KEY: &str = "event_id_sync_model_id";

fn should_request_events() -> bool {
    goose::get_worker_id() == 1
}

// accept option as goose manager builds the scenario as well, but doesn't need any peers and won't run it so it will always be Some in execution
pub async fn steady_sync_scenario(ipfs_peer_addr: Option<String>) -> Result<Scenario, GooseError> {
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

    let sync_event_id = transaction!(sync_event_id).set_name("sync_event_id");

    Ok(scenario!("SteadyEventIDSync")
        .register_transaction(test_start)
        .register_transaction(sync_event_id))
}

// send subscription to each node
// node A should send 1M events to node B
// node A must report how long it took to send 1M events
// node B must report how long it took from first request to last event
#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(
    user: &mut GooseUser,
    cli: CeramicClient,
    redis_cli: redis::Client,
    ipfs_peer_addr: Url,
) -> TransactionResult {
    let mut conn = redis_cli.get_async_connection().await.unwrap();
    let model_id = if should_request_events() {
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

    let user_data = ModelReuseLoadTestUserData {
        cli,
        redis_cli,
        model_id,
    };
    user.set_session_data(user_data);
    user.base_url = Some(ipfs_peer_addr); // Recon is only available on IPFS address right now

    let _subscribed_to_models = user
        .get_request_builder(
            &GooseMethod::Get,
            "/ceramic/subscribe/model/%7Bmodel%7D?limit=1",
        )?
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    Ok(())
}

async fn sync_event_id(user: &mut GooseUser) -> TransactionResult {
    if !should_request_events() {
        // this inflates the scenario/transaction metrics, but it doesn't affect the request metrics
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    } else {
        let user_data: &ModelReuseLoadTestUserData = user
            .get_session_data()
            .expect("we are missing sync_event_id user data");

        // eventId needs to be a multibase encoded string for the API to accept it
        let event_id = format!("F{}", random_event_id(&user_data.model_id.to_string()));
        let event_key_body = serde_json::json!({"eventId": event_id});
        tracing::debug!("sync_event_id body: {:?}", event_key_body);
        let request_builder = user
            .get_request_builder(&GooseMethod::Post, "/ceramic/events")?
            .timeout(Duration::from_secs(1))
            .json(&event_key_body);
        let req = GooseRequest::builder()
            .method(GooseMethod::Post)
            .set_request_builder(request_builder)
            .expect_status_code(204)
            .build();
        let mut goose = user.request(req).await?;
        let resp = goose.response?;
        if resp.status().is_success() {
            Ok(())
        } else {
            user.set_failure(
                "sync_event_id",
                &mut goose.request,
                None,
                Some(&format!("Failed to add event ID: {}", event_id)),
            )
        }
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
        &ceramic_core::Network::Local(42),
        SORT_KEY,
        sort_value,
        TEST_CONTROLLER,
        &cid,
        0,
        &cid,
    )
}
