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

use crate::network::{
    controller::{
        CAS_SERVICE_NAME, CERAMIC_APP, CERAMIC_SERVICE_API_PORT, CERAMIC_SERVICE_IPFS_PORT,
        CERAMIC_SERVICE_NAME, GANACHE_SERVICE_NAME, INIT_CONFIG_MAP_NAME,
    },
    utils::{ResourceLimitsConfig, ResourceLimitsSpec},
};

use crate::utils::{managed_labels, selector_labels};

const IPFS_CONTAINER_NAME: &str = "ipfs";
const IPFS_DATA_PV_CLAIM: &str = "ipfs-data";

pub fn config_maps(config: &CeramicConfig) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut config_maps = BTreeMap::new();
    if config.init_config_map == INIT_CONFIG_MAP_NAME {
        config_maps.insert(INIT_CONFIG_MAP_NAME.to_owned(),
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
        "disable-composedb": false,
        "enable-historical-sync": false
    }
}"#.to_owned()),
]));
    }
    config_maps.append(&mut config.ipfs.config_maps());
    config_maps
}

pub fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![
            ServicePort {
                port: CERAMIC_SERVICE_API_PORT,
                name: Some("api".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            },
            ServicePort {
                port: CERAMIC_SERVICE_IPFS_PORT,
                name: Some("ipfs".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            },
            ServicePort {
                port: 4001,
                name: Some("swarm-tcp".to_owned()),
                protocol: Some("TCP".to_owned()),
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
    /// Configuration of the IPFS container
    pub ipfs: Option<IpfsSpec>,
    /// Resource limits for ceramic nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
    /// Private key for signing anchor requests and generating the Admin DID.
    pub private_key: Option<String>,
}

/// Describes how the IPFS node for a peer should behave.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IpfsSpec {
    // Both Go and Rust have the same specification.
    // If this changes, its possible to convert IpfsSpec into an enum
    // with variants for each kind.
    // Until that need arises we use a `kind` field on the struct.
    pub kind: IpfsKind,
    pub image: Option<String>,
    pub image_pull_policy: Option<String>,
    // Resource limits for ipfs nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
}

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IpfsKind {
    #[default]
    Rust,
    Go,
}

pub struct CeramicConfig {
    pub init_config_map: String,
    pub ipfs: IpfsConfig,
    pub resource_limits: ResourceLimitsConfig,
}

pub enum IpfsConfig {
    Rust(RustIpfsConfig),
    Go(GoIpfsConfig),
}

impl Default for IpfsConfig {
    fn default() -> Self {
        Self::Rust(Default::default())
    }
}
impl IpfsConfig {
    fn config_maps(&self) -> BTreeMap<String, BTreeMap<String, String>> {
        match self {
            IpfsConfig::Rust(_) => BTreeMap::new(),
            IpfsConfig::Go(config) => config.config_maps(),
        }
    }
    fn into_container(self) -> Container {
        match self {
            IpfsConfig::Rust(config) => config.into_container(),
            IpfsConfig::Go(config) => config.into_container(),
        }
    }
    fn volumes(&self) -> Vec<Volume> {
        match self {
            IpfsConfig::Rust(_) => Vec::new(),
            IpfsConfig::Go(config) => config.volumes(),
        }
    }
}

pub struct RustIpfsConfig {
    image: String,
    image_pull_policy: String,
    resource_limits: ResourceLimitsConfig,
}

impl Default for RustIpfsConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
            resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("512Mi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
        }
    }
}
impl From<IpfsSpec> for RustIpfsConfig {
    fn from(value: IpfsSpec) -> Self {
        let default = RustIpfsConfig::default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
        }
    }
}

pub struct GoIpfsConfig {
    image: String,
    image_pull_policy: String,
    resource_limits: ResourceLimitsConfig,
}
impl Default for GoIpfsConfig {
    fn default() -> Self {
        Self {
            image: "ipfs/kubo:v0.19.1@sha256:c4527752a2130f55090be89ade8dde8f8a5328ec72570676b90f66e2cabf827d". to_owned(),
            image_pull_policy: "IfNotPresent".to_owned(),
            resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("512Mi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
        }
    }
}
impl From<IpfsSpec> for GoIpfsConfig {
    fn from(value: IpfsSpec) -> Self {
        let default = GoIpfsConfig::default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
        }
    }
}

impl Default for CeramicConfig {
    fn default() -> Self {
        Self {
            init_config_map: INIT_CONFIG_MAP_NAME.to_owned(),
            ipfs: IpfsConfig::default(),
            resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("1Gi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
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
            ipfs: value.ipfs.map(Into::into).unwrap_or(default.ipfs),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
        }
    }
}

impl From<IpfsSpec> for IpfsConfig {
    fn from(value: IpfsSpec) -> Self {
        match value.kind {
            IpfsKind::Rust => Self::Rust(value.into()),
            IpfsKind::Go => Self::Go(value.into()),
        }
    }
}

impl RustIpfsConfig {
    fn into_container(self) -> Container {
        Container {
            env: Some(vec![
                EnvVar {
                    name: "RUST_LOG".to_owned(),
                    value: Some("info,ceramic_one=debug,tracing_actix_web=debug".to_owned()),
                    ..Default::default()
                },
                EnvVar {
                    name: "CERAMIC_ONE_BIND_ADDRESS".to_owned(),
                    value: Some(format!("0.0.0.0:{CERAMIC_SERVICE_IPFS_PORT}")),
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
            ]),
            image: Some(self.image),
            image_pull_policy: Some(self.image_pull_policy),
            name: IPFS_CONTAINER_NAME.to_owned(),
            ports: Some(vec![
                ContainerPort {
                    container_port: 4001,
                    name: Some("swarm-tcp".to_owned()),
                    protocol: Some("TCP".to_owned()),
                    ..Default::default()
                },
                ContainerPort {
                    container_port: CERAMIC_SERVICE_IPFS_PORT,
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
                limits: Some(self.resource_limits.clone().into()),
                requests: Some(self.resource_limits.into()),
                ..Default::default()
            }),
            volume_mounts: Some(vec![VolumeMount {
                mount_path: "/data/ipfs".to_owned(),
                name: IPFS_DATA_PV_CLAIM.to_owned(),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }
}

impl GoIpfsConfig {
    fn config_maps(&self) -> BTreeMap<String, BTreeMap<String, String>> {
        BTreeMap::from_iter(vec![(
            "ipfs-container-init".to_owned(),
            BTreeMap::from_iter(vec![(
                "001-config.sh".to_owned(),
                r#"#!/bin/sh
set -ex
# Do not bootstrap against public nodes
ipfs bootstrap rm all
# Do not sticky peer with ceramic specific peers
# We want an isolated network
ipfs config --json Peering.Peers '[]'
# Disable the gateway
ipfs config  --json Addresses.Gateway '[]'
# Enable pubsub
ipfs config  --json PubSub.Enabled true
# Only listen on specific tcp address as nothing else is exposed
ipfs config  --json Addresses.Swarm '["/ip4/0.0.0.0/tcp/4001"]'
# Set explicit resource manager limits as Kubo computes them based off
# the k8s node resources and not the pods limits.
ipfs config Swarm.ResourceMgr.MaxMemory '400 MB'
ipfs config --json Swarm.ResourceMgr.MaxFileDescriptors 500000
"#
                .to_owned(),
            )]),
        )])
    }
    fn into_container(self) -> Container {
        Container {
            image: Some(self.image),
            image_pull_policy: Some(self.image_pull_policy),
            name: IPFS_CONTAINER_NAME.to_owned(),
            ports: Some(vec![
                ContainerPort {
                    container_port: 4001,
                    name: Some("swarm".to_owned()),
                    protocol: Some("TCP".to_owned()),
                    ..Default::default()
                },
                ContainerPort {
                    container_port: CERAMIC_SERVICE_IPFS_PORT,
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
                limits: Some(self.resource_limits.clone().into()),
                requests: Some(self.resource_limits.into()),
                ..Default::default()
            }),
            volume_mounts: Some(vec![
                VolumeMount {
                    mount_path: "/data/ipfs".to_owned(),
                    name: IPFS_DATA_PV_CLAIM.to_owned(),
                    ..Default::default()
                },
                VolumeMount {
                    mount_path: "/container-init.d/001-config.sh".to_owned(),
                    name: "ipfs-container-init".to_owned(),
                    // Use an explict subpath otherwise, k8s uses symlinks which breaks
                    // kubo's init logic.
                    sub_path: Some("001-config.sh".to_owned()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }
    }
    fn volumes(&self) -> Vec<Volume> {
        vec![Volume {
            name: "ipfs-container-init".to_owned(),
            config_map: Some(ConfigMapVolumeSource {
                default_mode: Some(0o755),
                name: Some("ipfs-container-init".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }]
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
            value: Some(format!("http://localhost:{CERAMIC_SERVICE_IPFS_PORT}")),
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

    let mut volumes = vec![
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
            name: IPFS_DATA_PV_CLAIM.to_owned(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: IPFS_DATA_PV_CLAIM.to_owned(),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    volumes.append(&mut config.ipfs.volumes());

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
                            container_port: CERAMIC_SERVICE_API_PORT,
                            name: Some("api".to_owned()),
                            ..Default::default()
                        }]),
                        readiness_probe: Some(Probe {
                            http_get: Some(HTTPGetAction {
                                path: Some("/api/v0/node/healthcheck".to_owned()),
                                port: IntOrString::String("api".to_owned()),
                                ..Default::default()
                            }),
                            initial_delay_seconds: Some(60),
                            period_seconds: Some(15),
                            timeout_seconds: Some(30),
                            ..Default::default()
                        }),
                        liveness_probe: Some(Probe {
                            http_get: Some(HTTPGetAction {
                                path: Some("/api/v0/node/healthcheck".to_owned()),
                                port: IntOrString::String("api".to_owned()),
                                ..Default::default()
                            }),
                            initial_delay_seconds: Some(60),
                            period_seconds: Some(15),
                            timeout_seconds: Some(30),
                            ..Default::default()
                        }),

                        resources: Some(ResourceRequirements {
                            limits: Some(config.resource_limits.clone().into()),
                            requests: Some(config.resource_limits.clone().into()),
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
                    config.ipfs.into_container(),
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
                        limits: Some(config.resource_limits.clone().into()),
                        requests: Some(config.resource_limits.into()),
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
                volumes: Some(volumes),
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
                    name: Some(IPFS_DATA_PV_CLAIM.to_owned()),
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
