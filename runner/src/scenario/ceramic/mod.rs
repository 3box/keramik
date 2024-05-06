pub mod anchor;
pub mod model_instance;
mod models;
pub mod new_streams;
pub mod query;
pub mod simple;
pub mod util;
pub mod write_only;

use ceramic_core::ssi::did::{DIDMethod, DocumentBuilder, Source};
use ceramic_core::ssi::jwk::{self, Base64urlUInt, Params, JWK};
use ceramic_http_client::ceramic_event::{DidDocument, JwkSigner};
use ceramic_http_client::CeramicHttpClient;

use models::RandomModelInstance;
use serde::{Deserialize, Serialize};

use crate::simulate::Scenario;

pub type CeramicClient = CeramicHttpClient<JwkSigner>;

#[derive(Clone)]
pub struct Credentials {
    pub signer: JwkSigner,
    pub did: DidDocument,
}

impl Credentials {
    pub async fn from_env() -> Result<Self, anyhow::Error> {
        let did = DidDocument::new(&std::env::var("DID_KEY").expect("DID_KEY is required"));
        let private_key = std::env::var("DID_PRIVATE_KEY").expect("DID_PRIVATE_KEY is required");
        let signer = JwkSigner::new(did.clone(), &private_key).await?;
        Ok(Self { signer, did })
    }

    pub async fn admin_from_env() -> Result<Self, anyhow::Error> {
        // TODO: move DID from private key to rust-ceramic
        // There is a private function (ed25519_parse_private) in spruceid ssi that does this.
        // it's possible I'm missing an easier way  that would avoid bringing in the dependency on ed25519_dalek
        let private_key = std::env::var("CERAMIC_ADMIN_PRIVATE_KEY")
            .expect("CERAMIC_ADMIN_PRIVATE_KEY is required");

        let data = hex::decode(&private_key)?;

        let key: ed25519_dalek::SigningKey = data[..].try_into()?;
        let key = JWK::from(Params::OKP(jwk::OctetParams {
            curve: "Ed25519".to_string(),
            public_key: Base64urlUInt(ed25519_dalek::VerifyingKey::from(&key).as_bytes().to_vec()),
            private_key: Some(Base64urlUInt(data.to_owned())),
        }));

        let did = Self::generate_did_for_jwk(&key)?;
        let signer = JwkSigner::new(did.clone(), &private_key).await?;
        Ok(Self { signer, did })
    }

    pub async fn new_generate_did_key() -> Result<Self, anyhow::Error> {
        let (pk, did) = Self::generate_did_and_pk()?;
        let signer = JwkSigner::new(did.clone(), &pk).await?;
        Ok(Self { signer, did })
    }

    fn generate_did_for_jwk(key: &JWK) -> anyhow::Result<DidDocument> {
        let did = did_method_key::DIDKey
            .generate(&Source::Key(key))
            .ok_or_else(|| anyhow::anyhow!("Failed to generate DID"))?;

        let doc: DidDocument = DocumentBuilder::default()
            .id(did)
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build DID document: {}", e))?;
        tracing::debug!("Generated DID: {:?}", doc);
        Ok(doc)
    }

    /// Returns (Private Key, DID Document)
    fn generate_did_and_pk() -> anyhow::Result<(String, DidDocument)> {
        let key = jwk::JWK::generate_ed25519()?;
        let private_key = if let Params::OKP(params) = &key.params {
            let pk = params
                .private_key
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No private key"))?;
            hex::encode(pk.0.as_slice())
        } else {
            anyhow::bail!("Invalid private key");
        };

        let did = Self::generate_did_for_jwk(&key)?;
        Ok((private_key, did))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DidType {
    /// One DID for all users
    Shared,
    /// A unique DID key for each user
    UserDidKey,
    // Use CACAOs for each user
    //UserCacao,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReuseType {
    /// Create a new model or model instance document for each user
    PerUser,
    /// Create a new model for each node (worker)
    PerNode,
    /// Reuse the same model or model instance document for all users
    Shared,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CeramicScenarioParameters {
    /// What type of DID signing to use. `UserDidKey` cannot be combined with `model_instance_reuse: Shared`
    pub did_type: DidType,
    /// Whether models should be shared or independent
    pub model_reuse: ReuseType,
    /// Whether MIDs should be shared (and updated simultaneously) or independent
    pub model_instance_reuse: ReuseType,
    /// How many model instance documents to create in advance for each model
    pub number_of_documents: usize,

    // If the MIDs should be stored in redis to use for later validation
    pub store_mids: bool,

    /// Whether to subscribe to all models created by the scenario on all nodes. This will increase sync load,
    /// particularly for scenarios like `CeramicNewStreams` where many MIDs are created on different nodes.
    pub subscribe_to_all_models: bool,
}

impl From<Scenario> for CeramicScenarioParameters {
    fn from(value: Scenario) -> Self {
        // did_type: DidType::UserDidKey and model_instance_reuse: ReuseType::Shared is an invalid combination
        // as we'll try to update documents owned by another controller and just log lots of errors
        match value {
            Scenario::CeramicSimple => Self {
                did_type: DidType::Shared,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
                store_mids: false,
                subscribe_to_all_models: true,
            },
            Scenario::CeramicModelReuse => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::Shared,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
                store_mids: false,
                subscribe_to_all_models: true,
            },
            Scenario::CeramicWriteOnly => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::Shared,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 1,
                store_mids: false,
                subscribe_to_all_models: true,
            },
            Scenario::CeramicNewStreams => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 0,
                store_mids: false,
                subscribe_to_all_models: true,
            },
            Scenario::ReconEventSync | Scenario::CeramicNewStreamsBenchmark => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::Shared,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 0,
                store_mids: false,
                subscribe_to_all_models: true,
            },
            Scenario::CeramicQuery => Self {
                did_type: DidType::Shared,
                model_reuse: ReuseType::PerUser,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 3,
                store_mids: false,
                subscribe_to_all_models: true,
            },
            Scenario::IpfsRpc | Scenario::CASBenchmark => {
                panic!("Not supported for non ceramic scenarios")
            }
            Scenario::CeramicAnchoringBenchmark => Self {
                did_type: DidType::UserDidKey,
                model_reuse: ReuseType::Shared,
                model_instance_reuse: ReuseType::PerUser,
                number_of_documents: 0,
                store_mids: true,
                subscribe_to_all_models: true,
            },
        }
    }
}
