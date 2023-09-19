use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

use crate::network::ResourceLimitsSpec;

#[derive(Clone)]
pub struct ResourceLimitsConfig {
    /// Cpu resource limit
    pub cpu: Quantity,
    /// Memory resource limit
    pub memory: Quantity,
    // Ephemeral storage resource limit
    pub storage: Quantity,
}

impl ResourceLimitsConfig {
    pub fn from_spec(spec: Option<ResourceLimitsSpec>, defaults: Self) -> Self {
        if let Some(spec) = spec {
            Self {
                cpu: spec.cpu.unwrap_or(defaults.cpu),
                memory: spec.memory.unwrap_or(defaults.memory),
                storage: spec.storage.unwrap_or(defaults.storage),
            }
        } else {
            defaults
        }
    }
}

impl From<ResourceLimitsConfig> for BTreeMap<String, Quantity> {
    fn from(value: ResourceLimitsConfig) -> Self {
        BTreeMap::from_iter([
            ("cpu".to_owned(), value.cpu),
            ("ephemeral-storage".to_owned(), value.storage),
            ("memory".to_owned(), value.memory),
        ])
    }
}
