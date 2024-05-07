use goose::prelude::*;
use ceramic::anchor::create_stream;
use ceramic_http_client::ceramic_event::{
    Cid, DidDocument, JwkSigner, Jws, StreamId, StreamIdType,
};
use tokio::sync::mpsc;

pub async fn data_feed_benchmark() -> Result<Scenario, GooseError> {
    //Must:
    // 1. Make a transaction that results in a data feed event could be genesis + updates
    let create_streams = 1;
    // 2. Increment TPS until data feed performance plateau
    // 3. Keep that metric and share
    Ok(scenario!("DataFeedBenchmark")
        .register_transaction(subscribe_to_data_feed)
        .register_transaction(perform_transaction))
}

async fn subscribe_to_data_feed(user: &GooseUser) {
    let url = "/api/v0/feed/aggregation/documents/";

    // Creates channel to communicate between the feed event handler and main thread
    let (tx, mut rx) = mpsc::channel::<String>(100);

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    tokio::spawn(async move {
        // Sets up a loop to receive SSE events
        while running_clone.load(Ordering::SeqCst) {
            // Simulates waiting for an event from the SSE endpoint
            tokio::select! {
                _ = interval(Duration::from_secs(1)) => {
                    // Generates a fake SSE event every second for testing
                    let event_data = "data: {\"event\": \"test_event\"}\n\n";
                    tx.send(event_data.to_string()).await.unwrap();
                }
            }
        }
    });

    // Makes a request to subscribe to the SSE endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(sse_url)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();

    // Checks if the SSE subscription was successful
    if response.status().is_success() {
        // Starts processing SSE events
        while let Some(event_data) = rx.recv().await {
            // Processes the received SSE event TODO
            println!("Received SSE event: {}", event_data);
        }
    } else {
        println!("Failed to subscribe to SSE endpoint: {}", response.status());
    }

    // Signal the background task to stop //TODO should this go in a stop function like in the recon one?
    running.store(false, Ordering::SeqCst);
}

async fn perform_transaction(user: &GooseUser) {
    // Simulate user performing a transaction
    // Keep track of transactions per second
}

pub fn create_transaction() -> TransactionResult {
    let (stream_id, genesis_cid, genesis_block) = create_stream(StreamIdType::Model).unwrap();
    Ok(())
}
/* 
pub fn read_event(user: &mut GooseUser, model_id: streamId) -> TransactionResult {
    let goose_request = GooseRequest::builder()
        .path("/api/v0/feed/aggregation/documents/")
        .name("feed-api")
        .build();

    // Make the configured request.
    let _goose = user.request(goose_request).await?;

    //Ok(())
    //let path = format!("/api/v0/feed/aggregation/documents/");
    let user_data = ReconCeramicModelInstanceTestUser {
        model_id,
        with_data: true,
    };
    user.set_session_data(user_data);

    /*let request_builder = user
        .get_request_builder(&GooseMethod::Post, &path)?
        .timeout(Duration::from_secs(5));
    let req = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(204)
        .build();

    let _goose = user.request(req).await?;*/
    Ok(())
}*/