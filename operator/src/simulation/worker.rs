use k8s_openapi::{
  api::{
      batch::v1::JobSpec,
      core::v1::{
          Container, EnvVar, PodSpec, PodTemplateSpec,
      },
  },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
            scenario: "ipfs-rpc".to_owned(),
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
                image: Some("keramik/runner".to_owned()),
                image_pull_policy: Some("Always".to_owned()),
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
                        value: Some("info,runner=trace".to_owned()),
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
                        name: "SIMULATE_TOTAL_PEERS".to_owned(),
                        value: Some(config.total_peers.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SIMULATE_NONCE".to_owned(),
                        value: Some(config.nonce.to_string()),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }],
            restart_policy: Some("Never".to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    },
    ..Default::default()
  }
}

