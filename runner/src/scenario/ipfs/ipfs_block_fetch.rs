use anyhow::Result;
use cid::Cid;
use goose::prelude::*;
use libipld::prelude::Codec;
use libipld::{ipld, json::DagJsonCodec};
use multihash::{Code, MultihashDigest};
use std::{sync::Arc, time::Duration};

use crate::simulate::Topology;

pub fn scenario(topo: Topology) -> Result<Scenario> {
    let put: Transaction = Transaction::new(Arc::new(move |user| {
        Box::pin(async move { put(topo, user).await })
    }))
    .set_name("dag_put")
    .set_on_start();

    let get: Transaction = Transaction::new(Arc::new(move |user| {
        Box::pin(async move { get(topo, user).await })
    }))
    .set_name("dag_get");

    let check: Transaction = Transaction::new(Arc::new(move |user| {
        Box::pin(async move { check(topo, user).await })
    }))
    .set_name("check")
    .set_on_stop();

    Ok(scenario!("IpfsRpc")
        // After each transactions runs, sleep randomly from 1 to 5 seconds.
        .set_wait_time(Duration::from_secs(1), Duration::from_secs(5))?
        // This transaction only runs one time when the user first starts.
        .register_transaction(put)
        // These next two transactions run repeatedly as long as the load test is running.
        .register_transaction(get)
        .register_transaction(check))
}

// Determine global unique id for user based on the worker id and total number of workers
fn global_user_id(user: usize, topo: Topology) -> u64 {
    ((topo.target_worker as u64) * (topo.total_workers as u64)) + (user as u64)
}

/// Produce DAG-JSON IPLD node that contains determisiticly unique data for the user.
fn user_data(local_user: usize, topo: Topology) -> (Cid, Vec<u8>) {
    let id = global_user_id(local_user, topo);
    let data = ipld!({
        "user": id,
        "nonce": topo.nonce,
    });

    let bytes = DagJsonCodec.encode(&data).unwrap();

    let hash = Code::Sha2_256.digest(bytes.as_slice());
    (Cid::new_v1(DagJsonCodec.into(), hash), bytes)
}

// Generate determisitic random data and put it into IPFS
async fn put(topo: Topology, user: &mut GooseUser) -> TransactionResult {
    let (cid, data) = user_data(user.weighted_users_index, topo);
    println!(
        "put id: {} user: {} nonce: {} cid: {}",
        topo.target_worker,
        user.weighted_users_index,
        topo.nonce,
        cid.to_string(),
    );

    // Build a Reqwest RequestBuilder object.
    let part = reqwest::multipart::Part::bytes(data);
    let form = reqwest::multipart::Form::new().part("file", part);

    // Use block put to ensure the cid remains the same.
    let path = "/api/v0/block/put?cid-codec=dag-json";
    let url = user.build_url(path)?;
    let reqwest_request_builder = user.client.post(url).multipart(form);

    // POST request.
    let goose_request = GooseRequest::builder()
        .method(GooseMethod::Post)
        .path(path)
        .set_request_builder(reqwest_request_builder)
        .expect_status_code(200)
        .build();

    // Make the request and return the GooseResponse.
    let goose = user.request(goose_request).await?;
    println!("{:?}", goose.response?.text().await);

    Ok(())
}

// Get CID from IPFS
async fn get(mut topo: Topology, user: &mut GooseUser) -> TransactionResult {
    // Always get the data for worker 0
    topo.target_worker = 0;
    let (cid, _data) = user_data(user.weighted_users_index, topo);
    println!(
        "get id: {} user: {} cid: {}",
        topo.target_worker,
        user.weighted_users_index,
        cid.to_string(),
    );

    let request_builder = user
        .get_request_builder(
            &GooseMethod::Post,
            format!("/api/v0/dag/get?arg={}", cid.to_string()).as_str(),
        )?
        .timeout(Duration::from_secs(5));

    // Manually build a GooseRequest.
    let goose_request = GooseRequest::builder()
        // Manually add our custom RequestBuilder object.
        .set_request_builder(request_builder)
        .expect_status_code(200)
        // Turn the GooseRequestBuilder object into a GooseRequest.
        .build();

    // Finally make the actual request with our custom GooseRequest object.
    let _goose = user.request(goose_request).await?;
    Ok(())
}

// Check that all written data is accounted for.
async fn check(topo: Topology, user: &mut GooseUser) -> TransactionResult {
    let (cid, data) = user_data(user.weighted_users_index, topo);
    println!(
        "stop id: {} user: {} cid: {}",
        topo.target_worker,
        user.weighted_users_index,
        cid.to_string(),
    );

    let request_builder = user
        .get_request_builder(
            &GooseMethod::Post,
            format!("/api/v0/dag/get?arg={}", cid.to_string()).as_str(),
        )?
        .timeout(Duration::from_secs(15));

    // Manually build a GooseRequest.
    let goose_request = GooseRequest::builder()
        // Manually add our custom RequestBuilder object.
        .set_request_builder(request_builder)
        .expect_status_code(200) // Turn the GooseRequestBuilder object into a GooseRequest.
        .build();

    // Finally make the actual request with our custom GooseRequest object.
    let mut goose = user.request(goose_request).await?;
    let body = goose.response?.bytes().await?;
    if body != data {
        return user.set_failure("user data missing", &mut goose.request, None, None);
    }
    Ok(())
}
