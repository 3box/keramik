use anyhow::Result;
use base64::{engine::general_purpose, Engine};
use ceramic_http_client::ceramic_event::{
    Cid, DidDocument, JwkSigner, Jws, StreamId, StreamIdType,
};
use chrono::Utc;
use goose::prelude::*;
use iroh_car::{CarHeader, CarWriter};
use libipld::{cbor::DagCborCodec, ipld, prelude::Codec, Ipld, IpldCodec};
use multihash::{Code::Sha2_256, MultihashDigest};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use redis::{aio::MultiplexedConnection, AsyncCommands};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::scenario::get_redis_client;

#[derive(Serialize, Deserialize)]
struct CasAuthPayload {
    url: String,
    nonce: String,
    digest: String,
}

async fn auth_header(url: String, controller: String, digest: Cid) -> Result<String> {
    let auth_payload = CasAuthPayload {
        url,
        nonce: Uuid::new_v4().to_string(),
        digest: digest.to_string(),
    };

    let node_private_key = std::env::var("NODE_PRIVATE_KEY").unwrap_or_else(|_| {
        "b650ae31651a33cc1a40402e5b38266b8b6eb9213013b3fa3a91b3f065ab643a".to_string()
    });
    let signer = JwkSigner::new(DidDocument::new(controller.as_str()), &node_private_key)
        .await
        .unwrap();
    let auth_jws = Jws::for_data(&signer, &auth_payload).await?;
    let (sig, protected) = auth_jws
        .signatures
        .first()
        .and_then(|sig| sig.protected.as_ref().map(|p| (&sig.signature, p)))
        .unwrap();
    Ok(format!("Bearer {}.{}.{}", protected, auth_jws.payload, sig))
}

pub async fn stream_tip_car(
    stream_id: StreamId,
    genesis_cid: Cid,
    genesis_block: Vec<u8>,
    tip_cid: Cid,
    tip_block: Vec<u8>,
) -> Result<(Cid, Vec<u8>)> {
    let root_block = ipld!({
        "timestamp": Utc::now().to_rfc3339(),
        "streamId": stream_id.to_vec()?,
        "tip": genesis_cid,
    });

    let ipld_bytes = DagCborCodec.encode(&root_block)?;
    let root_cid = Cid::new_v1(IpldCodec::DagCbor.into(), Sha2_256.digest(&ipld_bytes));
    let car_header = CarHeader::new_v1(vec![root_cid]);
    let mut car_writer = CarWriter::new(car_header, Vec::new());
    car_writer.write(root_cid, ipld_bytes).await.unwrap();
    car_writer
        .write(genesis_cid, genesis_block.clone())
        .await
        .unwrap();
    car_writer.write(tip_cid, tip_block).await.unwrap();
    Ok((root_cid, car_writer.finish().await.unwrap().to_vec()))
}

pub async fn create_anchor_request_on_cas(
    user: &mut GooseUser,
    conn: Arc<tokio::sync::Mutex<MultiplexedConnection>>,
) -> TransactionResult {
    let cas_service_url = std::env::var("CAS_SERVICE_URL")
        .unwrap_or_else(|_| "https://cas-dev.3boxlabs.com".to_string());
    let node_controller = std::env::var("node_controller")
        .unwrap_or_else(|_| "did:key:z6Mkh3pajt5brscshuDrCCber9nC9Ujpi7EcECveKtJPMEPo".to_string());
    let (stream_id, genesis_cid, genesis_block) = create_stream(StreamIdType::Tile).unwrap();

    let (root_cid, car_bytes) = stream_tip_car(
        stream_id.clone(),
        genesis_cid,
        genesis_block.clone(),
        genesis_cid,
        genesis_block,
    )
    .await
    .unwrap();

    let auth_header = auth_header(cas_service_url.clone(), node_controller.clone(), root_cid)
        .await
        .unwrap();
    let cas_create_request_url = cas_service_url.clone() + "/api/v0/requests";
    let builder = user
        .get_request_builder(&GooseMethod::Post, &cas_create_request_url)
        .unwrap();
    let goose_request = GooseRequest::builder()
        .name("create_anchor_request")
        .set_request_builder(
            builder
                .header("Authorization", auth_header)
                .header("Content-Type", "application/vnd.ipld.car")
                .body(car_bytes),
        )
        .expect_status_code(200)
        .build();

    let _response = user.request(goose_request).await?;
    let mut conn: tokio::sync::MutexGuard<'_, MultiplexedConnection> = conn.lock().await;
    let _: () = conn
        .sadd("anchor_requests", stream_id.to_string())
        .await
        .unwrap();
    Ok(())
}

pub async fn cas_benchmark() -> Result<Scenario, GooseError> {
    let redis_cli = get_redis_client().await.unwrap();
    let multiplexed_conn = redis_cli.get_multiplexed_tokio_connection().await.unwrap();
    let conn_mutex = Arc::new(Mutex::new(multiplexed_conn));
    let create_anchor_request = Transaction::new(Arc::new(move |user| {
        let conn_mutex_clone = conn_mutex.clone();
        Box::pin(async move { create_anchor_request_on_cas(user, conn_mutex_clone).await })
    }))
    .set_name("create_anchor_request");

    Ok(scenario!("CeramicCasBenchmark").register_transaction(create_anchor_request))
}

/// Create a new Ceramic stream
pub fn create_stream(stream_type: StreamIdType) -> Result<(StreamId, Cid, Vec<u8>)> {
    let controller: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let genesis_commit = ipld!({
        "header": {
            "unique": stream_unique_header(),
            "controllers": [controller]
        }
    });
    // Deserialize the genesis commit, encode it as CBOR, and compute the CID.
    let ipld_map: BTreeMap<String, Ipld> = libipld::serde::from_ipld(genesis_commit)?;
    let ipld_bytes = DagCborCodec.encode(&ipld_map)?;
    let genesis_cid = Cid::new_v1(IpldCodec::DagCbor.into(), Sha2_256.digest(&ipld_bytes));
    Ok((
        StreamId {
            r#type: stream_type,
            cid: genesis_cid,
        },
        genesis_cid,
        ipld_bytes,
    ))
}

fn stream_unique_header() -> String {
    let mut data = [0u8; 8];
    thread_rng().fill(&mut data);
    general_purpose::STANDARD.encode(data)
}
