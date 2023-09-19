use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{EnvVar, EnvVarSource, ObjectFieldSelector};

use crate::network::DataDogSpec;

/// Describes if and how to configure datadog telemetry
pub struct DataDogConfig {
    enabled: bool,
    version: String,
    profiling_enabled: bool,
}

impl DataDogConfig {
    pub fn inject_annotations(&self, annotations: &mut BTreeMap<String, String>) {
        if self.enabled {
            annotations.insert(
                "admission.datadoghq.com/js-lib.version".to_owned(),
                "latest".to_owned(),
            );
        }
    }
    pub fn inject_labels(
        &self,
        labels: &mut BTreeMap<String, String>,
        env: String,
        service: String,
    ) {
        if self.enabled {
            labels.insert("tags.datadoghq.com/env".to_owned(), env);
            labels.insert("tags.datadoghq.com/service".to_owned(), service);
            labels.insert(
                "tags.datadoghq.com/version".to_owned(),
                self.version.to_owned(),
            );
            labels.insert(
                "admission.datadoghq.com/enabled".to_owned(),
                "true".to_owned(),
            );
        }
    }
    pub fn inject_env(&self, env: &mut Vec<EnvVar>) {
        if self.enabled {
            env.push(EnvVar {
                name: "DD_AGENT_HOST".to_owned(),
                value_from: Some(EnvVarSource {
                    field_ref: Some(ObjectFieldSelector {
                        field_path: "status.hostIP".to_owned(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            });
            env.push(EnvVar {
                name: "DD_RUNTIME_METRICS_ENABLED".to_owned(),
                value: Some("true".to_owned()),
                ..Default::default()
            });
            if self.profiling_enabled {
                env.push(EnvVar {
                    name: "DD_PROFILING_ENABLED".to_owned(),
                    value: Some("true".to_owned()),
                    ..Default::default()
                });
            }
        }
    }
}
impl Default for DataDogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            version: "0".to_owned(),
            profiling_enabled: false,
        }
    }
}

impl From<&Option<DataDogSpec>> for DataDogConfig {
    fn from(value: &Option<DataDogSpec>) -> Self {
        let default = DataDogConfig::default();
        if let Some(value) = value {
            Self {
                enabled: value.enabled.unwrap_or(default.enabled),
                version: value.version.to_owned().unwrap_or(default.version),
                profiling_enabled: value.profiling_enabled.unwrap_or(default.profiling_enabled),
            }
        } else {
            default
        }
    }
}
