use goose::prelude::*;
use crate::scenario::get_redis_client;

use super::new_streams::instantiate_small_model;
use std::{sync::Arc, time::Instant};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::{sync::{mpsc, Semaphore, Mutex}, time::{interval, Duration}};

pub async fn data_feed_benchmark() -> Result<Scenario, GooseError> {
    //Must:
    // 1. Make a transaction that results in a data feed event could be genesis + updates
    let subscribe_sse = transaction!(process_data_feed);
    // 2. Increment TPS until data feed performance plateau
    // 3. Keep that metric and share
    Ok(scenario!("DataFeedBenchmark")
        .register_transaction(subscribe_sse))
}

async fn create_model(user: &mut GooseUser, trigger_tx: mpsc::Sender<()>) {
    let redis_cli = get_redis_client().await.unwrap();//TODO do i need redis here?
    let multiplexed_conn = redis_cli.get_multiplexed_tokio_connection().await.unwrap();
    let shared_conn = Arc::new(Mutex::new(multiplexed_conn));
    let small_model_conn = shared_conn.clone();
    // Call the function to instantiate a small model
    let result = instantiate_small_model(user, true, small_model_conn.clone()).await;

    match result {
        Ok(()) => trigger_tx.send(()).await.unwrap_or_else(|err| {
            // Handle the error, e.g., log it or return a default value
            println!("Error sending trigger message: {:?}", err);
        }),
        Err(_) => todo!(),
    } 
}

async fn process_data_feed(user: &mut GooseUser) -> TransactionResult {
    let url = "/api/v0/feed/aggregation/documents/";
    //TODO extract all the setup to a setup function
    // Creates channels to communicate between the feed event handler and main thread
    let (_tx, mut rx) = mpsc::channel::<String>(100);
    let (trigger_tx, mut trigger_rx) = mpsc::channel::<()>(100);

    // Semaphore to control access to the trigger channel until feed subscription is established
    let semaphore = Arc::new(Semaphore::new(0));

    // Tracks the number of triggers per second and events received per second
    let triggers_per_second = Arc::new(AtomicUsize::new(0));
    let events_per_second = Arc::new(AtomicUsize::new(0));

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let triggers_per_second_clone = triggers_per_second.clone();
    let events_per_second_clone = events_per_second.clone();
    let running_clone_outer = running_clone.clone();
    let triggers_per_second_clone_outer = triggers_per_second_clone.clone();
    let events_per_second_clone_outer = events_per_second_clone.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(1));
        while running_clone_outer.load(Ordering::SeqCst) {
            interval.tick().await;
            triggers_per_second_clone_outer.store(0, Ordering::Relaxed);
            events_per_second_clone_outer.store(0, Ordering::Relaxed);
        }
    });

    let semaphore_clone = semaphore.clone();
    tokio::spawn(async move {
        // Wait until the semaphore is released (SSE subscription is established)
        semaphore_clone.acquire().await.unwrap();
        while running_clone.load(Ordering::SeqCst) {
            let _ = trigger_rx.recv().await;
            triggers_per_second_clone.fetch_add(1, Ordering::Relaxed);
        }
    });

    // Makes a request to subscribe to the SSE endpoint
    let client = reqwest::Client::new();
    let start_time = Instant::now();
    let response = client
        .get(url)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();

    // Allows events triggering to start
    semaphore.add_permits(1);
    // Checks if the SSE subscription was successful
    if response.status().is_success() {
        // Start generating triggers (calls create_model to trigger SSE events)
        create_model(user, trigger_tx).await;
        // Starts processing SSE events
        while let Some(event_data) = rx.recv().await {
            events_per_second.fetch_add(1, Ordering::Relaxed);
            // Processes the received SSE event TODO
            println!("Received SSE event: {}", event_data);

            // Compare triggers per second with events received per second TODO other scenarios do this in simulate
            let elapsed_time = start_time.elapsed().as_secs_f64();
            let triggers = triggers_per_second.load(Ordering::Relaxed);
            let events = events_per_second.load(Ordering::Relaxed);
            let triggers_per_second = (triggers as f64) / elapsed_time;
            let events_per_second = (events as f64) / elapsed_time;
            println!("Triggers per second: {:.2}, Events per second: {:.2}", triggers_per_second, events_per_second);
        }
    } else {
        println!("Failed to subscribe to SSE endpoint: {}", response.status());
    }

    // Signal the background task to stop //TODO should this go in a stop function like in the recon one?
    running.store(false, Ordering::SeqCst);
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