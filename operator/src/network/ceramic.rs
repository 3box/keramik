use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::{RollingUpdateStatefulSetStrategy, StatefulSetSpec, StatefulSetUpdateStrategy},
        core::v1::{
            ConfigMapVolumeSource, Container, ContainerPort, EmptyDirVolumeSource, EnvVar,
            EnvVarSource, HTTPGetAction, PersistentVolumeClaim, PersistentVolumeClaimSpec,
            PersistentVolumeClaimVolumeSource, PodSpec, PodTemplateSpec, Probe,
            ResourceRequirements, SecretKeySelector, ServicePort, ServiceSpec, Volume, VolumeMount,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, util::intstr::IntOrString,
    },
};
use kube::core::ObjectMeta;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::network::controller::{
    managed_labels, selector_labels, CAS_SERVICE_NAME, CERAMIC_APP, CERAMIC_SERVICE_NAME,
    CERAMIC_SERVICE_RPC_PORT, GANACHE_SERVICE_NAME, INIT_CONFIG_MAP_NAME,
};

pub fn init_config_map_data() -> BTreeMap<String, String> {
    BTreeMap::from_iter(vec![
             ("ceramic-init.sh".to_owned(),
r#"#!/bin/bash

set -eo pipefail

export CERAMIC_ADMIN_DID=$(composedb did:from-private-key ${CERAMIC_ADMIN_PRIVATE_KEY})

CERAMIC_ADMIN_DID=$CERAMIC_ADMIN_DID envsubst < /ceramic-init/daemon-config.json > /config/daemon-config.json
"#.to_owned()),

("daemon-config.json".to_owned(),
r#"{
    "anchor": {},
    "http-api": {
        "cors-allowed-origins": [
            "${CERAMIC_CORS_ALLOWED_ORIGINS}"
        ],
        "admin-dids": [
            "${CERAMIC_ADMIN_DID}"
        ]
    },
    "ipfs": {
        "mode": "remote",
        "host": "${CERAMIC_IPFS_HOST}"
    },
    "logger": {
        "log-level": ${CERAMIC_LOG_LEVEL},
        "log-to-files": false
    },
    "metrics": {
        "metrics-exporter-enabled": false
    },
    "network": {
        "name": "${CERAMIC_NETWORK}",
        "pubsub-topic": "${CERAMIC_NETWORK_TOPIC}"
    },
    "node": {},
    "state-store": {
        "mode": "fs",
        "local-directory": "${CERAMIC_STATE_STORE_PATH}"
    },
    "indexing": {
        "db": "sqlite://${CERAMIC_SQLITE_PATH}",
        "allow-queries-before-historical-sync": true,
        "disable-composedb": true,
        "enable-historical-sync": false
    }
}"#.to_owned()),
])
}

pub fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![
            ServicePort {
                port: 7007,
                name: Some("ceramic".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            },
            ServicePort {
                port: CERAMIC_SERVICE_RPC_PORT,
                name: Some("rpc".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            },
            ServicePort {
                port: 4001,
                name: Some("swarm-tcp".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            },
            ServicePort {
                port: 4002,
                name: Some("swarm-udp".to_owned()),
                protocol: Some("UDP".to_owned()),
                ..Default::default()
            },
        ]),
        selector: selector_labels(CERAMIC_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}

/// Describes how a Ceramic peer should behave.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CeramicSpec {
    /// Name of a config map with a ceramic-init.sh script that runs as an initialization step.
    pub init_config_map: Option<String>,
}

pub struct CeramicConfig {
    pub init_config_map: String,
}
impl Default for CeramicConfig {
    fn default() -> Self {
        Self {
            init_config_map: INIT_CONFIG_MAP_NAME.to_owned(),
        }
    }
}
impl From<Option<CeramicSpec>> for CeramicConfig {
    fn from(value: Option<CeramicSpec>) -> Self {
        match value {
            Some(spec) => spec.into(),
            None => CeramicConfig::default(),
        }
    }
}

impl From<CeramicSpec> for CeramicConfig {
    fn from(value: CeramicSpec) -> Self {
        let default = Self::default();
        Self {
            init_config_map: value.init_config_map.unwrap_or(default.init_config_map),
        }
    }
}

pub fn stateful_set_spec(replicas: i32, config: impl Into<CeramicConfig>) -> StatefulSetSpec {
    let config = config.into();
    let ceramic_env = vec![
        EnvVar {
            name: "CERAMIC_NETWORK".to_owned(),
            value: Some("local".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_NETWORK_TOPIC".to_owned(),
            value: Some("/ceramic/local-keramik".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_SQLITE_PATH".to_owned(),
            value: Some("/ceramic-data/ceramic.db".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_STATE_STORE_PATH".to_owned(),
            value: Some("/ceramic-data/statestore".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_IPFS_HOST".to_owned(),
            value: Some(format!("http://localhost:{CERAMIC_SERVICE_RPC_PORT}")),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_CORS_ALLOWED_ORIGINS".to_owned(),
            value: Some(".*".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_LOG_LEVEL".to_owned(),
            value: Some("2".to_owned()),
            ..Default::default()
        },
    ];
    let mut init_env = vec![EnvVar {
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
    }];
    init_env.append(&mut ceramic_env.clone());

    let ipfs_env = vec![
        EnvVar {
            name: "RUST_LOG".to_owned(),
            value: Some("info,ceramic_one=debug,tracing_actix_web=debug".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_ONE_BIND_ADDRESS".to_owned(),
            value: Some(format!("0.0.0.0:{CERAMIC_SERVICE_RPC_PORT}")),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_ONE_METRICS".to_owned(),
            value: Some("true".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_ONE_METRICS_BIND_ADDRESS".to_owned(),
            value: Some("0.0.0.0:9090".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_ONE_SWARM_ADDRESSES".to_owned(),
            value: Some("/ip4/0.0.0.0/tcp/4001".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_ONE_STORE_DIR".to_owned(),
            value: Some("/data/ipfs".to_owned()),
            ..Default::default()
        },
    ];
    StatefulSetSpec {
        pod_management_policy: Some("Parallel".to_owned()),
        replicas: Some(replicas),
        selector: LabelSelector {
            match_labels: selector_labels(CERAMIC_APP),
            ..Default::default()
        },
        service_name: CERAMIC_SERVICE_NAME.to_owned(),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                annotations: Some(BTreeMap::from_iter(vec![(
                    "prometheus/path".to_owned(),
                    "/metrics".to_owned(),
                )])),
                labels: selector_labels(CERAMIC_APP).map(|mut lbls| {
                    lbls.append(&mut managed_labels().unwrap());
                    lbls
                }),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![
                    Container {
                        command: Some(vec![
                            "/js-ceramic/packages/cli/bin/ceramic.js".to_owned(),
                            "daemon".to_owned(),
                            "--config".to_owned(),
                            "/config/daemon-config.json".to_owned(),
                            "--anchor-service-api".to_owned(),
                            format!("http://{CAS_SERVICE_NAME}:8081"),
                            "--ethereum-rpc".to_owned(),
                            format!("http://{GANACHE_SERVICE_NAME}:8545"),
                        ]),
                        env: Some(ceramic_env),
                        image: Some("3boxben/composedb:latest".to_owned()),
                        image_pull_policy: Some("Always".to_owned()),
                        name: "ceramic".to_owned(),
                        ports: Some(vec![ContainerPort {
                            container_port: 7007,
                            name: Some("api".to_owned()),
                            ..Default::default()
                        }]),
                        readiness_probe: Some(Probe {
                            http_get: Some(HTTPGetAction {
                                path: Some("/api/v0/node/healthcheck".to_owned()),
                                port: IntOrString::String("api".to_owned()),
                                ..Default::default()
                            }),
                            initial_delay_seconds: Some(30),
                            period_seconds: Some(15),
                            ..Default::default()
                        }),
                        liveness_probe: Some(Probe {
                            http_get: Some(HTTPGetAction {
                                path: Some("/api/v0/node/healthcheck".to_owned()),
                                port: IntOrString::String("api".to_owned()),
                                ..Default::default()
                            }),
                            initial_delay_seconds: Some(30),
                            period_seconds: Some(15),
                            ..Default::default()
                        }),

                        resources: Some(ResourceRequirements {
                            limits: Some(BTreeMap::from_iter(vec![
                                ("cpu".to_owned(), Quantity("250m".to_owned())),
                                ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                                ("memory".to_owned(), Quantity("512Mi".to_owned())),
                            ])),
                            requests: Some(BTreeMap::from_iter(vec![
                                ("cpu".to_owned(), Quantity("250m".to_owned())),
                                ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                                ("memory".to_owned(), Quantity("512Mi".to_owned())),
                            ])),
                            ..Default::default()
                        }),
                        volume_mounts: Some(vec![
                            VolumeMount {
                                mount_path: "/config".to_owned(),
                                name: "config-volume".to_owned(),
                                ..Default::default()
                            },
                            VolumeMount {
                                mount_path: "/ceramic-data".to_owned(),
                                name: "ceramic-data".to_owned(),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    },
                    Container {
                        env: Some(ipfs_env),
                        image: Some("public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest".to_owned()),
                        image_pull_policy: Some("Always".to_owned()),
                        name: "ipfs".to_owned(),
                        ports: Some(vec![
                            ContainerPort {
                                container_port: 4001,
                                name: Some("swarm-tcp".to_owned()),
                                protocol: Some("TCP".to_owned()),
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: 4002,
                                name: Some("swarm-quic".to_owned()),
                                protocol: Some("UDP".to_owned()),
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: CERAMIC_SERVICE_RPC_PORT,
                                name: Some("rpc".to_owned()),
                                protocol: Some("TCP".to_owned()),
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: 9090,
                                name: Some("metrics".to_owned()),
                                protocol: Some("TCP".to_owned()),
                                ..Default::default()
                            },
                        ]),
                        resources: Some(ResourceRequirements {
                            limits: Some(BTreeMap::from_iter(vec![
                                ("cpu".to_owned(), Quantity("250m".to_owned())),
                                ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                                ("memory".to_owned(), Quantity("512Mi".to_owned())),
                            ])),
                            requests: Some(BTreeMap::from_iter(vec![
                                ("cpu".to_owned(), Quantity("250m".to_owned())),
                                ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                                ("memory".to_owned(), Quantity("512Mi".to_owned())),
                            ])),
                            ..Default::default()
                        }),
                        volume_mounts: Some(vec![VolumeMount {
                            mount_path: "/data/ipfs".to_owned(),
                            name: "ipfs-data".to_owned(),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    },
                ],
                init_containers: Some(vec![Container {
                    command: Some(vec![
                        "/bin/bash".to_owned(),
                        "-c".to_owned(),
                        "/ceramic-init/ceramic-init.sh".to_owned(),
                    ]),
                    env: Some(init_env),
                    image: Some("3boxben/composedb:latest".to_owned()),
                    image_pull_policy: Some("Always".to_owned()),
                    name: "init-ceramic-config".to_owned(),
                    resources: Some(ResourceRequirements {
                        limits: Some(BTreeMap::from_iter(vec![
                            ("cpu".to_owned(), Quantity("250m".to_owned())),
                            ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                            ("memory".to_owned(), Quantity("512Mi".to_owned())),
                        ])),
                        requests: Some(BTreeMap::from_iter(vec![
                            ("cpu".to_owned(), Quantity("250m".to_owned())),
                            ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                            ("memory".to_owned(), Quantity("512Mi".to_owned())),
                        ])),
                        ..Default::default()
                    }),
                    volume_mounts: Some(vec![
                        VolumeMount {
                            mount_path: "/config".to_owned(),
                            name: "config-volume".to_owned(),
                            ..Default::default()
                        },
                        VolumeMount {
                            mount_path: "/ceramic-init".to_owned(),
                            name: "ceramic-init".to_owned(),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }]),
                volumes: Some(vec![
                    Volume {
                        empty_dir: Some(EmptyDirVolumeSource::default()),
                        name: "config-volume".to_owned(),
                        ..Default::default()
                    },
                    Volume {
                        config_map: Some(ConfigMapVolumeSource {
                            default_mode: Some(0o755),
                            name: Some(config.init_config_map),
                            ..Default::default()
                        }),
                        name: "ceramic-init".to_owned(),
                        ..Default::default()
                    },
                    Volume {
                        name: "ceramic-data".to_owned(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: "ceramic-data".to_owned(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Volume {
                        name: "ipfs-data".to_owned(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: "ipfs-data".to_owned(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
        },
        update_strategy: Some(StatefulSetUpdateStrategy {
            rolling_update: Some(RollingUpdateStatefulSetStrategy {
                max_unavailable: Some(IntOrString::String("50%".to_owned())),
                ..Default::default()
            }),
            ..Default::default()
        }),
        volume_claim_templates: Some(vec![
            PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("ceramic-data".to_owned()),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_owned()]),
                    resources: Some(ResourceRequirements {
                        requests: Some(BTreeMap::from_iter(vec![(
                            "storage".to_owned(),
                            Quantity("10Gi".to_owned()),
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("ipfs-data".to_owned()),
                    ..Default::default()
                },
                spec: Some(PersistentVolumeClaimSpec {
                    access_modes: Some(vec!["ReadWriteOnce".to_owned()]),
                    resources: Some(ResourceRequirements {
                        requests: Some(BTreeMap::from_iter(vec![(
                            "storage".to_owned(),
                            Quantity("10Gi".to_owned()),
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ]),
        ..Default::default()
    }
}
