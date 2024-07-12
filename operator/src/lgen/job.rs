use std::collections::BTreeMap;

use k8s_openapi::api::{batch::v1::JobSpec, core::v1::{ConfigMapVolumeSource, Container, EnvVar, PodSpec, PodTemplateSpec, Volume, VolumeMount}};
use kube::api::ObjectMeta;
use crate::{lgen::spec::LoadGeneratorSpec, network::PEERS_CONFIG_MAP_NAME};

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
    pub name: String,
    pub scenario: String,
    pub users: u32,
    pub run_time: u32,
    pub throttle_requests: Option<usize>,
    pub nonce: u32,
    pub job_image_config: JobImageConfig,
}

pub fn job_spec(config: JobConfig) -> JobSpec {
    let env_vars = vec![
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
    ];


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
                        "generate_load".to_owned(),
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