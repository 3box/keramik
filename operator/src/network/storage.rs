use std::collections::BTreeMap;

use k8s_openapi::{
    api::core::v1::{PersistentVolumeClaimSpec, ResourceRequirements},
    apimachinery::pkg::api::resource::Quantity,
};

use crate::network::PersistentStorageSpec;

#[derive(Clone)]
pub struct PersistentStorageConfig {
    /// Persistent storage resource limit
    pub size: Quantity,
    pub class: Option<String>,
}

impl PersistentStorageConfig {
    pub fn from_spec(spec: Option<PersistentStorageSpec>, defaults: Self) -> Self {
        if let Some(spec) = spec {
            Self {
                size: spec.size.unwrap_or(defaults.size),
                class: spec.class.or(defaults.class),
            }
        } else {
            defaults
        }
    }
}

impl From<PersistentStorageConfig> for PersistentVolumeClaimSpec {
    fn from(value: PersistentStorageConfig) -> Self {
        Self {
            access_modes: Some(vec!["ReadWriteOnce".to_owned()]),
            resources: Some(ResourceRequirements {
                requests: Some(BTreeMap::from_iter(vec![(
                    "storage".to_owned(),
                    value.size,
                )])),
                ..Default::default()
            }),
            storage_class_name: value.class,
            ..Default::default()
        }
    }
}
