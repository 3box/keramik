use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Describes if and how to configure datadog telemetry
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataDogSpec {
    pub enabled: Option<bool>,
}

/// Describes if and how to configure datadog telemetry
pub struct DataDogConfig {
    pub enabled: bool,
}

impl Default for DataDogConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

impl From<&Option<DataDogSpec>> for DataDogConfig {
    fn from(value: &Option<DataDogSpec>) -> Self {
        let default = DataDogConfig::default();
        if let Some(value) = value {
            Self {
                enabled: value.enabled.unwrap_or(default.enabled),
            }
        } else {
            default
        }
    }
}
