use std::{collections::BTreeMap};

use k8s_openapi::{
    api::{
        batch::v1::JobSpec,
        core::v1::{
            Container, EnvVar, PodSpec, PodTemplateSpec, ServicePort, ServiceSpec,
        },
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rand::random;

pub fn service_spec() -> ServiceSpec {
  ServiceSpec {
      ports: Some(vec![
          ServicePort {
              port: 5115,
              name: Some("manager".to_owned()),
              ..Default::default()
          },
      ]),
      selector: Some(BTreeMap::from_iter(vec![(
        "name".to_owned(),
        "goose".to_owned(),
      )])),
      cluster_ip: Some("None".to_owned()),
      ..Default::default()
  }
}

/// ManagerSpec defines a goose manager
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSpec {
    pub scenario: Option<String>,
    pub total_peers: Option<u32>,
    pub users: Option<u32>,
    pub run_time: Option<u32>,
    pub nonce: Option<u32>,
}

// ManagerConfig defines which properties of the JobSpec can be customized.
pub struct ManagerConfig {
    pub scenario: String,
    pub total_peers: u32,
    pub users: u32,
    pub run_time: u32,
    pub nonce: u32,
}

// Define clear defaults for this config
impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            scenario: "ipfs-rpc".to_owned(),
            total_peers: 1,
            users: 100,
            run_time: 10,
            nonce: random::<u32>(),
        }
    }
}

impl From<Option<ManagerSpec>> for ManagerConfig {
    fn from(value: Option<ManagerSpec>) -> Self {
        match value {
            Some(spec) => spec.into(),
            None => ManagerConfig::default(),
        }
    }
}

impl From<ManagerSpec> for ManagerConfig {
    fn from(value: ManagerSpec) -> Self {
        let default = Self::default();
        Self {
            scenario: value.scenario.unwrap_or(default.scenario),
            total_peers: value.total_peers.unwrap_or(default.total_peers),
            users: value.users.unwrap_or(default.users),
            run_time: value.run_time.unwrap_or(default.run_time),
            nonce: value.nonce.unwrap_or(default.nonce),
        }
    }
}

pub fn manager_job_spec(config: impl Into<ManagerConfig>) -> JobSpec {
  let config = config.into();
  JobSpec {
    backoff_limit: Some(4),
    template: PodTemplateSpec {
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "manager".to_owned(),
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
                        value: Some(1.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                        name: "SIMULATE_TOTAL_PEERS".to_owned(),
                        value: Some(config.total_peers.to_string()),
                        ..Default::default()
                    },
                    // TODO what does this do, pass opt?
                    EnvVar {
                        name: "SIMULATE_NONCE".to_owned(),
                        value: Some(10986711.to_string()),
                        ..Default::default()
                    },
                    EnvVar {
                      name: "SIMULATE_USERS".to_owned(),
                      value: Some(config.users.to_string()),
                      ..Default::default()
                  },
                    EnvVar {
                      name: "SIMULATE_RUN_TIME".to_owned(),
                      value: Some(config.run_time.to_string()),
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
