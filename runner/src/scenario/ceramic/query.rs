use crate::goose_try;
use crate::scenario::ceramic::model_instance::ModelInstanceRequests;
use crate::scenario::ceramic::models::LargeModel;
use ceramic_http_client::api::StreamsResponse;
use ceramic_http_client::ceramic_event::StreamId;
use ceramic_http_client::{FilterQuery, OperationFilter};
use goose::metrics::GooseRequestMetric;
use goose::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

use super::model_instance::{CeramicModelInstanceTestUser, EnvBasedConfig};
use super::CeramicScenarioParameters;

#[derive(Clone)]
struct QueryLoadTestUserData {
    model_instance_id1: StreamId,
    model_instance_id2: StreamId,
    model_instance_id3: StreamId,
    instance1_starting_int_value: i64,
    instance2_starting_int_value: i64,
    instance3_starting_int_value: i64,
}

impl QueryLoadTestUserData {
    fn model_instance_id_for_user(&self, user: &GooseUser) -> &StreamId {
        match user.weighted_users_index % 3 {
            0 => &self.model_instance_id1,
            1 => &self.model_instance_id2,
            _ => &self.model_instance_id3,
        }
    }

    fn int_value_for_user(&self, user: &GooseUser) -> i64 {
        let start = match user.weighted_users_index % 3 {
            0 => self.instance1_starting_int_value,
            1 => self.instance2_starting_int_value,
            _ => self.instance3_starting_int_value,
        };
        let increment = user.get_iterations() % 2 == 0;
        if increment {
            start + 1
        } else {
            start - 1
        }
    }
}

pub async fn scenario(params: CeramicScenarioParameters) -> Result<Scenario, GooseError> {
    let config = CeramicModelInstanceTestUser::prep_scenario(params)
        .await
        .unwrap();
    let test_start = Transaction::new(Arc::new(move |user| Box::pin(setup(user, config.clone()))))
        .set_name("setup")
        .set_on_start();

    let pre_query_models =
        transaction!(query_models_pre_update).set_name("pre_update_query_models");
    let update_models = transaction!(update_models).set_name("update_models");
    let post_query_models =
        transaction!(query_models_post_update).set_name("post_update_query_models");

    Ok(scenario!("CeramicQueryScenario")
        .register_transaction(test_start)
        .register_transaction(pre_query_models)
        .register_transaction(update_models)
        .register_transaction(post_query_models))
}

#[instrument(skip_all, fields(user.index = user.weighted_users_index), ret)]
async fn setup(user: &mut GooseUser, config: EnvBasedConfig) -> TransactionResult {
    CeramicModelInstanceTestUser::setup_scenario(user, config)
        .await
        .unwrap();

    let data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    // we copy the data here just to make it simpler to work with in the tests
    // the CeramicModelInstanceTestUser probably has too much logic built in already.. we don't _always_ need
    // large and small models, but it seems fine to setup thing for MIDs consistently.. and we make assumptions
    // about the values set in the model instance documents for the large model
    let user_data = QueryLoadTestUserData {
        model_instance_id1: data.large_model_instance_ids.first().unwrap().clone(),
        model_instance_id2: data.large_model_instance_ids.get(1).unwrap().clone(),
        model_instance_id3: data.large_model_instance_ids.get(2).unwrap().clone(),
        instance1_starting_int_value: 10,
        instance2_starting_int_value: 20,
        instance3_starting_int_value: 30,
    };

    user.set_session_data(user_data);

    Ok(())
}

async fn query_large_mid_verify_edges(
    user: &mut GooseUser,
    filter: FilterQuery,
) -> Result<(GooseRequestMetric, LargeModel), TransactionError> {
    let user_data: Arc<CeramicModelInstanceTestUser> =
        CeramicModelInstanceTestUser::user_data(user).to_owned();
    let (resp, mut metrics) = ModelInstanceRequests::query_model(
        user,
        user_data.user_cli(),
        &user_data.large_model_id,
        Some(filter),
    )
    .await?;

    if resp.edges.first().is_none() {
        goose_try!(user, "query", &mut metrics, {
            Err(anyhow::anyhow!("no edges returned"))
        })?;
    }
    let resp: LargeModel = goose_try!(user, "query", &mut metrics, {
        resp.edges
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no edges returned"))
            .and_then(|edge| serde_json::from_value(edge.node.content).map_err(anyhow::Error::from))
    })?;

    Ok((metrics, resp))
}

async fn query_models_pre_update(user: &mut GooseUser) -> TransactionResult {
    let mut where_filter = HashMap::new();
    where_filter.insert(
        "description".to_string(),
        OperationFilter::EqualTo("1".into()),
    );
    let filter = FilterQuery::Where(where_filter);
    query_large_mid_verify_edges(user, filter).await?;

    Ok(())
}

async fn get_large_mid_verify_edges(
    user: &mut GooseUser,
    stream_id: &StreamId,
    name: &str,
) -> Result<(StreamsResponse, LargeModel), TransactionError> {
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();

    let (resp, mut metrics) =
        ModelInstanceRequests::get_stream(user, user_data.user_cli(), stream_id, name).await?;
    let data: LargeModel = goose_try!(user, &format!("verify_edges_{}", name), &mut metrics, {
        resp.state
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No content"))
            .and_then(|st| serde_json::from_value(st.content.clone()).map_err(anyhow::Error::from))
    })?;

    Ok((resp, data))
}

async fn update_models(user: &mut GooseUser) -> TransactionResult {
    let (new_value, model_id) = {
        let data: &QueryLoadTestUserData = user.get_session_data_unchecked();
        let new_value = data.int_value_for_user(user);
        let model_id = data.model_instance_id_for_user(user).clone();
        (new_value, model_id)
    };

    let name = "update_models";
    let user_data = CeramicModelInstanceTestUser::user_data(user).to_owned();
    let cli = &user_data.user_cli();

    let (resp, mut data) = get_large_mid_verify_edges(user, &model_id, name).await?;
    data.tpe = new_value;

    ModelInstanceRequests::replace_stream_tx(user, cli, &model_id, &resp, name, &data).await?;
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
    let (mut metrics, resp) = query_large_mid_verify_edges(user, filter).await?;

    if resp.tpe != expected_value {
        goose_try!(user, "query", &mut metrics, {
            Err(anyhow::anyhow!("field not updated"))
        })?;
    }
    Ok(())
}
