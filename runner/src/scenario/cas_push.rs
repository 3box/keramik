use anyhow::Result;
use goose::prelude::*;
use serde::Serialize;
use std::sync::Arc;

use crate::simulate::Topology;

pub fn scenario(topo: Topology) -> Result<Scenario> {
    // The first worker should pretend to be cas
    if topo.target_worker == 0 {
        scenario_psuedo_cas(topo)
    } else {
        scenario_ipfs(topo)
    }
}

// Scenario to be a vanilla ipfs node syncing events
// Subscribes to a determisitic range of model events
pub fn scenario_ipfs(topo: Topology) -> Result<Scenario> {
    let subscribe: Transaction = Transaction::new(Arc::new(move |user| {
        Box::pin(async move { subscribe(topo, user).await })
    }))
    .set_name("subscribe")
    .set_on_start();

    Ok(scenario!("CasPushIpfs")
        // This transaction only runs one time when the user first starts.
        .register_transaction(subscribe))
}

// Scenario to pretend to be cas pushing events
// Pushes events to the entire range causing the rust-ceramic process to do Recon for those events.
pub fn scenario_psuedo_cas(topo: Topology) -> Result<Scenario> {
    let push: Transaction = Transaction::new(Arc::new(move |user| {
        Box::pin(async move { push(topo, user).await })
    }))
    .set_name("push");

    Ok(scenario!("IpfsRpc").register_transaction(push))
}

// Subscribe to a single model
async fn subscribe(_topo: Topology, user: &mut GooseUser) -> TransactionResult {
    let model = "a";
    let path = format!("/ceramic/subscribe/models/{model}?limit=1");
    let url = user.build_url(&path)?;
    let goose = user.get(&url).await?;
    goose.response?.json().await?;
    Ok(())
}

// Push events for all models
async fn push(_topo: Topology, user: &mut GooseUser) -> TransactionResult {
    #[derive(Serialize)]
    struct Request {
        #[serde(rename = "eventId")]
        event_id: String,
    }
    let req = Request {
        event_id: multibase::encode(multibase::Base::Base64, "a"),
    };
    let data: Vec<u8> = serde_json::to_vec(&req).unwrap();
    let path = "/ceramic/events";
    let url = user.build_url(path)?;
    let goose = user.post(&url, data).await?;
    goose.response?.json().await?;
    Ok(())
}
