use k8s_openapi::api::{
    batch::v1::JobSpec,
    core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, PodSpec, PodTemplateSpec, Volume, VolumeMount,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::network::controller::PEERS_CONFIG_MAP_NAME;

/// BootstrapSpec defines how the network bootstrap process should proceed.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSpec {
    /// Image of the runner for the bootstrap job.
    pub image: Option<String>,
    /// Image pull policy for the bootstrap job.
    pub image_pull_policy: Option<String>,
    /// Bootstrap method. Defaults to ring.
    pub method: Option<String>,
    /// Number of nodes to connect to each peer.
    pub n: Option<i32>,
}

// BootstrapConfig defines which properties of the JobSpec can be customized.
pub struct BootstrapConfig {
    pub image: String,
    pub image_pull_policy: String,
    pub method: String,
    pub n: i32,
}

// Define clear defaults for this config
impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/keramik-runner".to_owned(),
            image_pull_policy: "Always".to_owned(),
            method: "sentinel".to_owned(),
            n: 3,
        }
    }
}

impl From<Option<BootstrapSpec>> for BootstrapConfig {
    fn from(value: Option<BootstrapSpec>) -> Self {
        match value {
            Some(spec) => spec.into(),
            None => BootstrapConfig::default(),
        }
    }
}

impl From<BootstrapSpec> for BootstrapConfig {
    fn from(value: BootstrapSpec) -> Self {
        let default = Self::default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            method: value.method.unwrap_or(default.method),
            n: value.n.unwrap_or(default.n),
        }
    }
}

pub fn bootstrap_job_spec(config: impl Into<BootstrapConfig>) -> JobSpec {
    let config = config.into();
    JobSpec {
        backoff_limit: Some(4),
        template: PodTemplateSpec {
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "bootstrap".to_owned(),
                    image: Some(config.image),
                    image_pull_policy: Some(config.image_pull_policy),
                    command: Some(vec![
                        "/usr/bin/keramik-runner".to_owned(),
                        "bootstrap".to_owned(),
                    ]),
                    env: Some(vec![
                        EnvVar {
                            name: "RUNNER_OTLP_ENDPOINT".to_owned(),
                            value: Some("http://otel:4317".to_owned()),
                            ..Default::default()
                        },
                        EnvVar {
                            name: "RUST_LOG".to_owned(),
                            value: Some("info,keramik_runner=debug".to_owned()),
                            ..Default::default()
                        },
                        EnvVar {
                            name: "BOOTSTRAP_METHOD".to_owned(),
                            value: Some(config.method.to_owned()),
                            ..Default::default()
                        },
                        EnvVar {
                            name: "BOOTSTRAP_N".to_owned(),
                            value: Some(config.n.to_string()),
                            ..Default::default()
                        },
                        EnvVar {
                            name: "BOOTSTRAP_PEERS_PATH".to_owned(),
                            value: Some("/keramik-peers/peers.json".to_owned()),
                            ..Default::default()
                        },
                    ]),
                    volume_mounts: Some(vec![VolumeMount {
                        mount_path: "/keramik-peers".to_owned(),
                        name: "keramik-peers".to_owned(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                volumes: Some(vec![Volume {
                    config_map: Some(ConfigMapVolumeSource {
                        default_mode: Some(0o755),
                        name: Some(PEERS_CONFIG_MAP_NAME.to_owned()),
                        ..Default::default()
                    }),
                    name: "keramik-peers".to_owned(),
                    ..Default::default()
                }]),
                restart_policy: Some("Never".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}
