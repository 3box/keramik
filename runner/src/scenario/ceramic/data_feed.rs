use goose::prelude::*;
use ceramic::anchor::create_stream;
use ceramic_http_client::ceramic_event::{
    Cid, DidDocument, JwkSigner, Jws, StreamId, StreamIdType,
};

pub async fn data_feed_benchmark() -> Result<Scenario, GooseError> {
    //Must:
    // 1. Make a transaction that results in a data feed event could be genesis + updates
    let create_streams = 1;
    // 2. Increment TPS until data feed performance plateau
    // 3. Keep that metric and share
    Ok(())
}

pub fn create_transaction() -> TransactionResult {
    let (stream_id, genesis_cid, genesis_block) = create_stream(StreamIdType::Model).unwrap();
    Ok(())
}

pub fn read_event(user: &mut GooseUser, model_id: streamId) -> TransactionResult {
    let path = format!("/api/v0/feed/aggregation/documents/");
    let user_data = ReconCeramicModelInstanceTestUser {
        model_id,
        with_data: true,
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