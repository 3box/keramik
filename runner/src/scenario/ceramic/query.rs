use crate::goose_try;
use crate::scenario::ceramic::models::LargeModel;
use crate::scenario::ceramic::util::{goose_error, index_model, setup_model, setup_model_instance};
use crate::scenario::ceramic::{CeramicClient, Credentials};
use ceramic_http_client::api::{Pagination, StreamsResponse, StreamsResponseOrError};
use ceramic_http_client::ceramic_event::{JwkSigner, StreamId};
use ceramic_http_client::{
    api, CeramicHttpClient, FilterQuery, ModelAccountRelation, ModelDefinition, OperationFilter,
};
use goose::prelude::*;
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};
use tracing::instrument;

#[derive(Clone)]
struct QueryLoadTestUserData {
    cli: CeramicHttpClient<JwkSigner>,
    model_id: StreamId,
    model_instance_id1: StreamId,
    model_instance_id2: StreamId,
    model_instance_id3: StreamId,
}

const INSTANCE1_STARTING_INT_VALUE: i64 = 10;
const INSTANCE2_STARTING_INT_VALUE: i64 = 20;
const INSTANCE3_STARTING_INT_VALUE: i64 = 30;

impl QueryLoadTestUserData {
    fn model_id_for_user(&self, user: &GooseUser) -> &StreamId {
        match user.weighted_users_index % 3 {
            0 => &self.model_instance_id1,
            1 => &self.model_instance_id2,
            _ => &self.model_instance_id3,
        }
    }

    fn int_value_for_user(&self, user: &GooseUser) -> i64 {
        let start = match user.weighted_users_index % 3 {
            0 => INSTANCE1_STARTING_INT_VALUE,
            1 => INSTANCE2_STARTING_INT_VALUE,
            _ => INSTANCE3_STARTING_INT_VALUE,
        };
        let increment = user.get_iterations() % 2 == 0;
        if increment {
            start + 1
        } else {
            start - 1
        }
    }
}

pub async fn scenario() -> Result<Scenario, GooseError> {
    let creds = Credentials::from_env().await.map_err(goose_error)?;
    let cli = CeramicHttpClient::new(creds.signer);

    let test_start = Transaction::new(Arc::new(move |user| {
        Box::pin(setup(user, cli.clone()))
    }))
    .set_name("setup")
    .set_on_start();

    let pre_query_models =
        transaction!(query_models_pre_update).set_name("pre_update_query_models");
    let update_models = transaction!(update_models).set_name("update_models");
    let post_query_models =
        transaction!(query_models_post_update).set_name("post_update_query_models");

    Ok(scenario!("CeramicQueryScenario")
        // After each transactions runs, sleep randomly from 1 to 5 seconds.
        .set_wait_time(Duration::from_secs(1), Duration::from_secs(5))?
        .register_transaction(test_start)
        .register_transaction(pre_query_models)
        .register_transaction(update_models)
        .register_transaction(post_query_models))
}

#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(user: &mut GooseUser, cli: CeramicClient) -> TransactionResult {
    let model_definition =
        ModelDefinition::new::<LargeModel>("load_test_query_model", ModelAccountRelation::List)
            .unwrap();
    let model_id = setup_model(user, &cli, model_definition).await?;
    index_model(user, &cli, &model_id).await?;

    let id1 = setup_model_instance(
        user,
        &cli,
        &model_id,
        &LargeModel {
            creator: "keramik".to_string(),
            name: "load-test-query-model-1".to_string(),
            description: "a".to_string(),
            tpe: INSTANCE1_STARTING_INT_VALUE,
        },
    )
    .await?;
    let id2 = setup_model_instance(
        user,
        &cli,
        &model_id,
        &LargeModel {
            creator: "keramik".to_string(),
            name: "load-test-query-model-2".to_string(),
            description: "b".to_string(),
            tpe: INSTANCE2_STARTING_INT_VALUE,
        },
    )
    .await?;
    let id3 = setup_model_instance(
        user,
        &cli,
        &model_id,
        &LargeModel {
            creator: "keramik".to_string(),
            name: "load-test-query-model-3".to_string(),
            description: "c".to_string(),
            tpe: INSTANCE3_STARTING_INT_VALUE,
        },
    )
    .await?;

    let user_data = QueryLoadTestUserData {
        cli,
        model_id,
        model_instance_id1: id1,
        model_instance_id2: id2,
        model_instance_id3: id3,
    };

    user.set_session_data(user_data);

    Ok(())
}

async fn query_models_pre_update(user: &mut GooseUser) -> TransactionResult {
    let mut where_filter = HashMap::new();
    where_filter.insert(
        "description".to_string(),
        OperationFilter::EqualTo("a".into()),
    );
    let filter = FilterQuery::Where(where_filter);
    let user_data: &QueryLoadTestUserData = user.get_session_data_unchecked();
    let req = user_data
        .cli
        .create_query_request(&user_data.model_id, Some(filter), Pagination::default())
        .await
        .unwrap();
    let cli = &user_data.cli;
    let mut goose = user
        .request(
            GooseRequest::builder()
                .method(GooseMethod::Post)
                .set_request_builder(
                    user.client
                        .post(user.build_url(cli.collection_endpoint())?)
                        .json(&req),
                )
                .expect_status_code(200)
                .build(),
        )
        .await?;
    let resp: api::QueryResponse = goose.response?.json().await?;
    if resp.edges.first().is_none() {
        goose_try!(user, "query", &mut goose.request, {
            Err(anyhow::anyhow!("no edges returned"))
        })?;
    }
    Ok(())
}

async fn get_data_and_response<'a>(
    user: &'a mut GooseUser,
    req: GooseRequest<'a>,
    new_value: i64,
) -> Result<(StreamsResponse, LargeModel), TransactionError> {
    let mut goose = user.request(req).await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    let resp = goose_try!(user, "get", &mut goose.request, {
        resp.resolve("update_large_model_get")
    })?;
    let mut data: LargeModel = goose_try!(user, "get", &mut goose.request, {
        resp.state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No content"))
            .and_then(|st| serde_json::from_value(st.content.clone()).map_err(anyhow::Error::from))
    })?;
    data.tpe = new_value;
    Ok((resp, data))
}

async fn update_models(user: &mut GooseUser) -> TransactionResult {
    let user_data = {
        let data: &QueryLoadTestUserData = user.get_session_data_unchecked();
        data.clone()
    };
    let new_value = user_data.int_value_for_user(user);
    let model_id = user_data.model_id_for_user(user);
    let cli = &user_data.cli;
    let streams_url = user.build_url(&format!("{}/{}", cli.streams_endpoint(), model_id))?;
    let req = GooseRequest::builder()
        .method(GooseMethod::Get)
        .set_request_builder(user.client.get(streams_url))
        .expect_status_code(200)
        .build();
    let (resp, data) = get_data_and_response(user, req, new_value).await?;

    let req = cli
        .create_replace_request(model_id, &resp, data)
        .await
        .unwrap();

    let mut goose = user
        .request(
            GooseRequest::builder()
                .method(GooseMethod::Post)
                .set_request_builder(
                    user.client
                        .post(user.build_url(cli.commits_endpoint()).unwrap())
                        .json(&req),
                )
                .expect_status_code(200)
                .build(),
        )
        .await?;
    let resp: StreamsResponseOrError = goose.response?.json().await?;
    goose_try!(
        user,
        "update",
        &mut goose.request,
        resp.resolve("update_large_model")
    )?;
    Ok(())
}

async fn query_models_post_update(user: &mut GooseUser) -> TransactionResult {
    let user_data: &QueryLoadTestUserData = user.get_session_data_unchecked();

    let expected_value = user_data.int_value_for_user(user);

    let mut where_filter = HashMap::new();
    where_filter.insert(
        "tpe".to_string(),
        OperationFilter::EqualTo(expected_value.into()),
    );
    let filter = FilterQuery::Where(where_filter);

    let req = user_data
        .cli
        .create_query_request(&user_data.model_id, Some(filter), Pagination::default())
        .await
        .unwrap();
    let cli = &user_data.cli;
    let mut goose = user
        .request(
            GooseRequest::builder()
                .method(GooseMethod::Post)
                .set_request_builder(
                    user.client
                        .post(user.build_url(cli.collection_endpoint())?)
                        .json(&req),
                )
                .expect_status_code(200)
                .build(),
        )
        .await?;
    let resp: api::QueryResponse = goose.response?.json().await?;
    let resp: LargeModel = goose_try!(user, "query", &mut goose.request, {
        resp.edges
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no edges returned"))
            .and_then(|edge| serde_json::from_value(edge.node.content).map_err(anyhow::Error::from))
    })?;
    if resp.tpe != expected_value {
        goose_try!(user, "query", &mut goose.request, {
            Err(anyhow::anyhow!("field not updated"))
        })?;
    }
    Ok(())
}
