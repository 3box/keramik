use k8s_openapi::{
  api::{
      batch::v1::JobSpec,
      core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, PodSpec, PodTemplateSpec, Volume, VolumeMount,
      },
  },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::network::controller::PEERS_CONFIG_MAP_NAME;

/// WorkerSpec defines a goose worker 
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSpec {
    pub scenario: Option<String>,
    pub target_peer: Option<u32>,
    pub total_peers: Option<u32>,
    pub nonce: Option<u32>,
}

// WorkerConfig defines which properties of the JobSpec can be customized.
pub struct WorkerConfig {
    pub scenario: String,
    pub target_peer: u32,
    pub total_peers: u32,
    pub nonce: u32,
}

// Define clear defaults for this config
impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            scenario: "ceramic-simple".to_owned(),
            target_peer: 0,
            total_peers: 1,
            nonce: 1
        }
    }
}

impl From<Option<WorkerSpec>> for WorkerConfig {
    fn from(value: Option<WorkerSpec>) -> Self {
        match value {
            Some(spec) => spec.into(),
            None => WorkerConfig::default(),
        }
    }
}

impl From<WorkerSpec> for WorkerConfig {
    fn from(value: WorkerSpec) -> Self {
        let default = Self::default();
        Self {
            scenario: value.scenario.unwrap_or(default.scenario),
            target_peer: value.target_peer.unwrap_or(default.target_peer),
            total_peers: value.total_peers.unwrap_or(default.total_peers),
            nonce: value.nonce.unwrap_or(default.nonce),
        }
    }
}

pub fn worker_job_spec(config: impl Into<WorkerConfig>) -> JobSpec {
  let config = config.into();
  JobSpec {
    backoff_limit: Some(4),
    template: PodTemplateSpec {
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "worker".to_owned(),
                image: Some("keramik/runner:dev".to_owned()),
                image_pull_policy: Some("IfNotPresent".to_owned()),
                command: Some(vec![
                    "/usr/bin/keramik-runner".to_owned(), 
                    "simulate".to_owned(),
                ]),
                env: Some(vec![
                    EnvVar {
                        name: "RUNNER_OTLP_ENDPOINT".to_owned(),
                        value: Some("http://otel:4317".to_owned()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "RUST_LOG".to_owned(),
                        value: Some("info,keramik_runner=trace".to_owned()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "RUST_BACKTRACE".to_owned(),
                        value: Some("1".to_owned()),
                        ..Default::default()
                    },

                    EnvVar {
                        name: "SIMULATE_SCENARIO".to_owned(),
                        value: Some(config.scenario.to_owned()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SIMULATE_TARGET_PEER".to_owned(),
                        value: Some(config.target_peer.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SIMULATE_PEERS_PATH".to_owned(),
                        value: Some("/keramik-peers/peers.json".to_owned()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SIMULATE_NONCE".to_owned(),
                        value: Some(config.nonce.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "DID_KEY".to_owned(),
                        value: Some("did:key:z6Mkqn5jbycThHcBtakJZ8fHBQ2oVRQhXQEdQk5ZK2NDtNZA".to_owned()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "DID_PRIVATE_KEY".to_owned(),
                        value: Some("86dce513cf0a37d4acd6d2c2e00fe4b95e0e655ca51e1a890808f5fa6f4fe65a".to_owned()),
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

