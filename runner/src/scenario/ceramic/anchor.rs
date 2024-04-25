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

pub async fn create_anchor_request_on_cas(user: &mut GooseUser) -> TransactionResult {
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
    let data = user.get_session_data_unchecked::<CasData>();
    let mut conn = data.conn.lock().await;
    let _: () = conn
        .sadd("anchor_requests", stream_id.to_string())
        .await
        .map_err(|e| {
            tracing::error!("Failed to write to redis: {}", e);
            e
        })
        .unwrap();
    Ok(())
}

struct CasData {
    _redis_cli: redis::Client,
    conn: Arc<Mutex<MultiplexedConnection>>,
}

pub async fn cas_benchmark() -> Result<Scenario, GooseError> {
    let setup = transaction!(setup).set_name("setup_cas").set_on_start();
    let req = transaction!(create_anchor_request_on_cas).set_name("create_anchor_request_tx");

    Ok(scenario!("CeramicCasBenchmark")
        .register_transaction(setup)
        .register_transaction(req))
}

async fn setup(user: &mut GooseUser) -> TransactionResult {
    let redis_cli = get_redis_client().await.unwrap();
    let conn = Arc::new(Mutex::new(
        redis_cli.get_multiplexed_tokio_connection().await.unwrap(),
    ));
    user.set_session_data(CasData {
        _redis_cli: redis_cli,
        conn,
    });
    Ok(())
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
