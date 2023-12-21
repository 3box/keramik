use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

use crate::network::ResourceLimitsSpec;

#[derive(Clone)]
pub struct ResourceLimitsConfig {
    /// Cpu resource limit
    pub cpu: Option<Quantity>,
    /// Memory resource limit
    pub memory: Option<Quantity>,
    // Ephemeral storage resource limit
    pub storage: Quantity,
}

impl ResourceLimitsConfig {
    pub fn from_spec(spec: Option<ResourceLimitsSpec>, defaults: Self) -> Self {
        if let Some(spec) = spec {
            Self {
                cpu: spec.cpu,
                memory: spec.memory,
                storage: spec.storage.unwrap_or(defaults.storage),
            }
        } else {
            defaults
        }
    }
}

impl From<ResourceLimitsConfig> for BTreeMap<String, Quantity> {
    fn from(value: ResourceLimitsConfig) -> Self {
        let mut map = BTreeMap::from_iter([("ephemeral-storage".to_owned(), value.storage)]);
        value.cpu.and_then(|cpu| map.insert("cpu".to_owned(), cpu));
        value
            .memory
            .and_then(|memory| map.insert("memory".to_owned(), memory));
        map
    }
}
