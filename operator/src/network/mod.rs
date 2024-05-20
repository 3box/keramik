//! Network is k8s custom resource that defines a Ceramic network.

// Export all spec types
mod spec;
pub use spec::*;

// All other mods are behind the controller flag to keep the deps to a minimum
#[cfg(feature = "controller")]
pub(crate) mod bootstrap;
#[cfg(feature = "controller")]
pub(crate) mod cas;
#[cfg(feature = "controller")]
pub(crate) mod ceramic;
#[cfg(feature = "controller")]
pub(crate) mod controller;
#[cfg(feature = "controller")]
pub(crate) mod datadog;
#[cfg(feature = "controller")]
pub(crate) mod ipfs;
#[cfg(feature = "controller")]
pub(crate) mod ipfs_rpc;
#[cfg(feature = "controller")]
mod node_affinity;
#[cfg(feature = "controller")]
pub(crate) mod peers;
#[cfg(feature = "controller")]
pub(crate) mod resource_limits;
#[cfg(feature = "controller")]
pub(crate) mod storage;

#[cfg(test)]
#[cfg(feature = "controller")]
pub mod stub;

// Expose Context for testing
#[cfg(test)]
#[cfg(feature = "controller")]
pub use crate::utils::Context;

#[cfg(feature = "controller")]
pub use controller::{run, PEERS_CONFIG_MAP_NAME};

// Construct a SecurityContext for the debug mode spec setting.
// To be used by any containers that we might need to debug (i.e. containers running software we
// produce).
#[cfg(feature = "controller")]
fn debug_mode_security_context() -> k8s_openapi::api::core::v1::SecurityContext {
    k8s_openapi::api::core::v1::SecurityContext {
        capabilities: Some(k8s_openapi::api::core::v1::Capabilities {
            add: Some(vec!["SYS_PTRACE".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    }
}
