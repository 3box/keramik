use std::collections::BTreeMap;

use k8s_openapi::api::{
    batch::v1::JobSpec,
    core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec,
        SecretKeySelector, ServicePort, ServiceSpec, Volume, VolumeMount,
    },
};
use kube::core::ObjectMeta;

use crate::{network::PEERS_CONFIG_MAP_NAME, simulation::job::JobImageConfig};

pub fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            port: 5115,
            name: Some("manager".to_owned()),
            ..Default::default()
        }]),
        selector: Some(BTreeMap::from_iter(vec![(
            "name".to_owned(),
            "goose".to_owned(),
        )])),
        cluster_ip: Some("None".to_owned()),
        ..Default::default()
    }
}

// ManagerConfig defines which properties of the JobSpec can be customized.
pub struct ManagerConfig {
    pub name: String,
    pub scenario: String,
    pub users: u32,
    pub run_time: u32,
    pub throttle_requests: Option<usize>,
    pub nonce: u32,
    pub job_image_config: JobImageConfig,
    pub success_request_target: Option<usize>,
}

pub fn manager_job_spec(config: ManagerConfig) -> JobSpec {
    let mut env_vars = vec![
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
            name: "SIMULATE_NAME".to_owned(),
            value: Some(config.name.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_SCENARIO".to_owned(),
            value: Some(config.scenario.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_MANAGER".to_owned(),
            value: Some("true".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_PEERS_PATH".to_owned(),
            value: Some("/keramik-peers/peers.json".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_TARGET_PEER".to_owned(),
            value: Some(0.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_NONCE".to_owned(),
            value: Some(config.nonce.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_USERS".to_owned(),
            value: Some(config.users.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "SIMULATE_RUN_TIME".to_owned(),
            value: Some(format!("{}m", config.run_time)),
            ..Default::default()
        },
        EnvVar {
            name: "DID_KEY".to_owned(),
            value: Some("did:key:z6Mkqn5jbycThHcBtakJZ8fHBQ2oVRQhXQEdQk5ZK2NDtNZA".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "DID_PRIVATE_KEY".to_owned(),
            value: Some(
                "86dce513cf0a37d4acd6d2c2e00fe4b95e0e655ca51e1a890808f5fa6f4fe65a".to_owned(),
            ),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_ADMIN_PRIVATE_KEY".to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: "private-key".to_owned(),
                    name: Some("ceramic-admin".to_owned()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];
    if let Some(throttle_requests) = config.throttle_requests {
        env_vars.push(EnvVar {
            name: "SIMULATE_THROTTLE_REQUESTS".to_owned(),
            value: Some(throttle_requests.to_string()),
            ..Default::default()
        })
    }
    if let Some(success_request_target) = config.success_request_target {
        env_vars.push(EnvVar {
            name: "SIMULATE_TARGET_REQUESTS".to_owned(),
            value: Some(success_request_target.to_string()),
            ..Default::default()
        })
    }
    JobSpec {
        backoff_limit: Some(1),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some(BTreeMap::from_iter(vec![(
                    "name".to_owned(),
                    "goose".to_owned(),
                )])),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                hostname: Some("manager".to_owned()),
                subdomain: Some("goose".to_owned()),
                containers: vec![Container {
                    name: "manager".to_owned(),
                    image: Some(config.job_image_config.image),
                    image_pull_policy: Some(config.job_image_config.image_pull_policy),
                    command: Some(vec![
                        "/usr/bin/keramik-runner".to_owned(),
                        "simulate".to_owned(),
                    ]),
                    env: Some(env_vars),
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
        },
        ..Default::default()
    }
}
