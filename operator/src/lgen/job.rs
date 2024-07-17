use std::collections::BTreeMap;

use crate::{lgen::spec::LoadGeneratorSpec, network::PEERS_CONFIG_MAP_NAME};
use k8s_openapi::api::{
    batch::v1::JobSpec,
    core::v1::{
        ConfigMapVolumeSource, Container, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec,
        SecretKeySelector, Volume, VolumeMount,
    },
};
use kube::api::ObjectMeta;

/// Configuration for job images.
#[derive(Clone, Debug)]
pub struct JobImageConfig {
    /// Image for all jobs created by the load generator.
    pub image: String,
    /// Pull policy for image.
    pub image_pull_policy: String,
}

impl Default for JobImageConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/keramik-runner:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
        }
    }
}

impl From<&LoadGeneratorSpec> for JobImageConfig {
    fn from(value: &LoadGeneratorSpec) -> Self {
        let default = Self::default();
        Self {
            image: value.image.to_owned().unwrap_or(default.image),
            image_pull_policy: value
                .image_pull_policy
                .to_owned()
                .unwrap_or(default.image_pull_policy),
        }
    }
}

/// JobConfig defines which properties of the JobSpec can be customized.
pub struct JobConfig {
    /// Name of the load generator job.
    pub name: String,
    /// Scenario to run.
    pub scenario: String,
    /// Number of tasks to run.
    pub tasks: usize,
    /// Run time in seconds.
    pub run_time: u32,
    /// Throttle requests rate.
    pub throttle_requests: Option<usize>,
    /// Nonce for the load generator.
    pub nonce: u32,
    /// Image configuration for the load generator job.
    pub job_image_config: JobImageConfig,
}

/// Create a job spec for the load generator.
pub fn job_spec(config: JobConfig) -> JobSpec {
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
            name: "GENERATOR_NAME".to_owned(),
            value: Some(config.name.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "GENERATOR_SCENARIO".to_owned(),
            value: Some(config.scenario.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "GENERATOR_TASKS".to_owned(),
            value: Some(config.tasks.to_owned().to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "GENERATOR_RUN_TIME".to_owned(),
            value: Some(config.run_time.to_owned().to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "GENERATOR_NONCE".to_owned(),
            value: Some(config.nonce.to_owned().to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "GENERATOR_PEERS_PATH".to_owned(),
            value: Some("/keramik-peers/peers.json".to_owned()),
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
            name: "GENERATOR_THROTTLE_REQUESTS".to_owned(),
            value: Some(throttle_requests.to_string()),
            ..Default::default()
        })
    }

    JobSpec {
        backoff_limit: Some(1),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some(BTreeMap::from_iter(vec![(
                    "name".to_owned(),
                    "load-gen-job".to_owned(),
                )])),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                hostname: Some("job".to_owned()),
                subdomain: Some("load-gen-job".to_owned()),
                containers: vec![Container {
                    name: "job".to_owned(),
                    image: Some(config.job_image_config.image),
                    image_pull_policy: Some(config.job_image_config.image_pull_policy),
                    command: Some(vec![
                        "/usr/bin/keramik-runner".to_owned(),
                        "generate-load".to_owned(),
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
