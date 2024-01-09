pub mod event_id_sync;
pub mod model_reuse;
mod models;
pub mod new_streams;
pub mod query;
pub mod simple;
pub mod util;
pub mod write_only;

use ceramic_http_client::api::StreamsResponseOrError;
use ceramic_http_client::ceramic_event::{DidDocument, JwkSigner};
use ceramic_http_client::CeramicHttpClient;

use models::RandomModelInstance;

pub type CeramicClient = CeramicHttpClient<JwkSigner>;

#[derive(Clone)]
pub struct Credentials {
    pub signer: JwkSigner,
    pub did: DidDocument,
}

impl Credentials {
    pub async fn from_env() -> Result<Self, anyhow::Error> {
        let did = DidDocument::new(&std::env::var("DID_KEY").unwrap());
        let private_key = std::env::var("DID_PRIVATE_KEY").unwrap();
        let signer = JwkSigner::new(did.clone(), &private_key).await?;
        Ok(Self { signer, did })
    }
}
