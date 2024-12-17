use std::io::Write;

use ceramic_core::{Cid, DagCborEncoded, MultiBase32String};
use ceramic_http_client::ceramic_event::unvalidated;
use goose::GooseError;
use multihash_codetable::{Code, MultihashDigest};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use unsigned_varint::encode;

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

pub(crate) const DAG_CBOR_CODEC: u64 = 0x71;

/// Create a new Ceramic stream
pub fn create_stream() -> anyhow::Result<(
    ceramic_http_client::ceramic_event::StreamId,
    Cid,
    DagCborEncoded,
)> {
    let controller: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let genesis_commit = ipld_core::ipld!({
        "header": {
            "unique": gen_rand_bytes::<12>().as_slice(),
            "controllers": [controller]
        }
    });

    let bytes = DagCborEncoded::new(&genesis_commit)?;
    let cid = Cid::new_v1(DAG_CBOR_CODEC, Code::Sha2_256.digest(bytes.as_ref()));

    let stream_id = write_stream_bytes(&cid)?;
    let stream_id = ceramic_http_client::ceramic_event::StreamId::try_from(stream_id.as_slice())?;
    Ok((stream_id, cid, bytes))
}

const STREAMID_CODEC: u64 = 206;

pub fn write_stream_bytes(cid: &Cid) -> anyhow::Result<Vec<u8>> {
    let mut writer = std::io::BufWriter::new(Vec::new());
    let mut buf = encode::u64_buffer();
    let v = encode::u64(STREAMID_CODEC, &mut buf);
    writer.write_all(v)?;
    let v = encode::u64(3, &mut buf); // Model instance doc
    writer.write_all(v)?;
    cid.write_bytes(&mut writer)?;
    writer.flush()?;
    Ok(writer.into_inner()?)
}

pub(crate) async fn random_init_event_car(
    model: Vec<u8>,
    controller: Option<String>,
) -> Result<MultiBase32String, anyhow::Error> {
    let controller = if let Some(owner) = controller {
        owner
    } else {
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect()
    };

    let unique = gen_rand_bytes::<12>();
    let res = unvalidated::Builder::init()
        .with_controller(controller)
        .with_sep("model".to_string(), model)
        .with_unique(unique.to_vec())
        .with_data(ipld_core::ipld!({"a": 1, "b": 2}))
        .build();
    let car = res.encode_car()?;
    Ok(MultiBase32String::from(car))
}

fn gen_rand_bytes<const SIZE: usize>() -> [u8; SIZE] {
    // can't take &mut rng cause of Send even if we drop it
    let mut rng = thread_rng();
    let mut arr = [0; SIZE];
    for x in &mut arr {
        *x = rng.gen_range(0..=255);
    }
    arr
}
