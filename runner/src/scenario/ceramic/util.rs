use ceramic_core::{
    ssi::{
        did::{DIDMethod, DocumentBuilder, Source},
        jwk::{self, Params},
    },
    DidDocument,
};

use goose::GooseError;

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

/// Returns (Private Key, DID Document)
pub fn generate_did_and_pk() -> anyhow::Result<(String, DidDocument)> {
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

    let did = did_method_key::DIDKey
        .generate(&Source::Key(&key))
        .ok_or_else(|| anyhow::anyhow!("Failed to generate DID"))?;

    let doc: DidDocument = DocumentBuilder::default()
        .id(did)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build DID document: {}", e))?;
    tracing::debug!("Generated DID: {:?}", doc);
    Ok((private_key, doc))
}
