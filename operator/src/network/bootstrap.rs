use k8s_openapi::api::{
    batch::v1::JobSpec,
    core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, PodSpec, PodTemplateSpec, Volume, VolumeMount,
    },
};

use crate::{
    network::{node_affinity::NodeAffinityConfig, BootstrapSpec, PEERS_CONFIG_MAP_NAME},
    network_log_format,
};

// BootstrapConfig defines which properties of the JobSpec can be customized.
pub struct BootstrapConfig {
    pub enabled: bool,
    pub image: String,
    pub image_pull_policy: String,
    pub method: String,
    pub n: i32,
}

// Define clear defaults for this config
impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
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
            enabled: value.enabled.unwrap_or(default.enabled),
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            method: value.method.unwrap_or(default.method),
            n: value.n.unwrap_or(default.n),
        }
    }
}

pub fn bootstrap_job_spec(
    config: BootstrapConfig,
    node_affinity_config: &NodeAffinityConfig,
) -> JobSpec {
    debug_assert!(config.enabled);
    JobSpec {
        backoff_limit: Some(4),
        template: node_affinity_config.apply_to_pod_template(PodTemplateSpec {
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
                        EnvVar {
                            name: "RUNNER_LOG_FORMAT".to_owned(),
                            value: Some(network_log_format().to_string()),
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
        }),
        ..Default::default()
    }
}
