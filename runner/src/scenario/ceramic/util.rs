use crate::scenario::ceramic::CeramicClient;
use ceramic_http_client::{api, ceramic_event::StreamId, ModelDefinition};
use goose::goose::{GooseMethod, GooseRequest, GooseUser};
use goose::prelude::TransactionError;
use goose::GooseError;

pub fn goose_error(err: anyhow::Error) -> GooseError {
    GooseError::Io(std::io::Error::new(std::io::ErrorKind::Other, err))
}

/// Macro to transform errors from an expression to a goose transaction failiure
#[macro_export]
macro_rules! goose_try {
    ($user:ident, $tag:expr, $request:expr, $func:expr) => {
        match $func {
            Ok(ret) => Ok(ret),
            Err(e) => {
                let err = e.to_string();
                if let Err(e) = $user.set_failure($tag, $request, None, Some(&err)) {
                    Err(e)
                } else {
                    panic!("Unreachable")
                }
            }
        }
    };
}

pub async fn setup_model(
    user: &mut GooseUser,
    cli: &CeramicClient,
    model: ModelDefinition,
) -> Result<StreamId, TransactionError> {
    let url = user.build_url(cli.streams_endpoint())?;
    let req = cli.create_model_request(&model).await.unwrap();
    let req = user.client.post(url).json(&req);
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(req)
        .expect_status_code(200)
        .build();
    let mut goose = user.request(req).await?;
    let resp: api::StreamsResponseOrError = goose.response?.json().await?;
    let resp = goose_try!(user, "setup_model", &mut goose.request, {
        resp.resolve("setup_model")
    })?;
    Ok(resp.stream_id)
}

pub async fn setup_model_instance<T: serde::Serialize>(
    user: &mut GooseUser,
    cli: &CeramicClient,
    model: &StreamId,
    data: &T,
) -> Result<StreamId, TransactionError> {
    let url = user.build_url(cli.streams_endpoint())?;
    let req = cli.create_list_instance_request(model, data).await.unwrap();
    let req = user.client.post(url).json(&req);
    let req = GooseRequest::builder()
        .method(GooseMethod::Post)
        .set_request_builder(req)
        .expect_status_code(200)
        .build();
    let mut goose = user.request(req).await?;
    let resp: api::StreamsResponseOrError = goose.response?.json().await?;
    let resp = goose_try!(user, "setup_model_instance", &mut goose.request, {
        resp.resolve("setup_model_instance")
    })?;
    Ok(resp.stream_id)
}

pub async fn index_model(
    user: &mut GooseUser,
    cli: &CeramicClient,
    model_id: &StreamId,
) -> Result<(), TransactionError> {
    let result = user
        .request(
            GooseRequest::builder()
                .method(GooseMethod::Get)
                .set_request_builder(user.client.get(user.build_url(cli.admin_code_endpoint())?))
                .expect_status_code(200)
                .build(),
        )
        .await?;
    let resp: api::AdminCodeResponse = result.response?.json().await?;
    let req = cli
        .create_index_model_request(model_id, &resp.code)
        .await
        .unwrap();
    let mut goose = user
        .request(
            GooseRequest::builder()
                .method(GooseMethod::Post)
                .set_request_builder(
                    user.client
                        .post(user.build_url(cli.index_endpoint())?)
                        .json(&req),
                )
                .expect_status_code(200)
                .build(),
        )
        .await?;
    let resp = goose.response?;
    if resp.status().is_success() {
        Ok(())
    } else {
        user.set_failure(
            "index_model",
            &mut goose.request,
            None,
            Some(&format!("Failed to index model: {}", resp.text().await?)),
        )
    }
}
