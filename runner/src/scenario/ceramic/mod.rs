pub mod model_instance;
pub mod model_reuse;
mod models;
pub mod new_streams;
pub mod query;
pub mod recon_sync;
pub mod simple;
pub mod util;
pub mod write_only;

use ceramic_http_client::ceramic_event::{DidDocument, JwkSigner};
use ceramic_http_client::CeramicHttpClient;

use models::RandomModelInstance;

use crate::simulate::Scenario;

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

    pub async fn new_generate_did_key() -> Result<Self, anyhow::Error> {
        let (pk, did) = util::generate_did_and_pk()?;
        let signer = JwkSigner::new(did.clone(), &pk).await?;
        Ok(Self { signer, did })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DidType {
    /// One DID for all users
    Shared,
    /// A unique DID key for each user
    UserDidKey,
    // Use CACAOs for each user
    //UserCacao,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReuseType {
    /// Create a new model or model instance document for each user
    PerUser,
    // Create a new model for each node (worker)
    // PerNode,
    /// Reuse the same model or model instance document for all users
    Shared,
}

#[derive(Debug, Clone)]
pub struct CeramicScenarioParameters {
    pub did_type: DidType,
    /// Whether models should be shared or independent
    pub model_reuse: ReuseType,
    /// How many model instance documents to create in advance for each model.
    pub model_instance_reuse: ReuseType,
    pub number_of_documents: usize,
}

impl From<Scenario> for CeramicScenarioParameters {
    fn from(value: Scenario) -> Self {
        match value {
            Scenario::CeramicSimple => Self {
                did_type: DidType::Shared,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
            },
            Scenario::CeramicUserSimple => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
            },
            Scenario::CeramicWriteOnly => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
            },
            Scenario::CeramicNewStreams => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
            },
            Scenario::CeramicQuery => Self {
                did_type: DidType::Shared,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 3,
            },
            Scenario::CeramicModelReuse => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::Shared,
                model_instance_reuse: ReuseType::Shared,
                number_of_documents: 1,
            },
            Scenario::IpfsRpc | Scenario::ReconEventSync | Scenario::ReconEventKeySync => {
                panic!("Not supported for non ceramic scenarios")
            }
        }
    }
}
