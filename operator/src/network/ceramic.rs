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
        NetworkConfig, CERAMIC_APP, CERAMIC_SERVICE_API_PORT, CERAMIC_SERVICE_IPFS_PORT,
        INIT_CONFIG_MAP_NAME,
    },
    datadog::DataDogConfig,
    utils::{ResourceLimitsConfig, ResourceLimitsSpec},
};

use crate::utils::{managed_labels, selector_labels};

const IPFS_CONTAINER_NAME: &str = "ipfs";
const IPFS_DATA_PV_CLAIM: &str = "ipfs-data";

pub fn config_maps(
    info: &CeramicInfo,
    config: &CeramicConfig,
) -> BTreeMap<String, BTreeMap<String, String>> {
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
    "anchor": {
        "auth-method": "did",
        "anchor-service-url": "${CAS_API_URL}",
        "ethereum-rpc-url": "${ETH_RPC_URL}"
    },
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
        "metrics-exporter-enabled": false,
        "prometheus-exporter-enabled": true,
        "prometheus-exporter-port": 9464
    },
    "network": {
        "name": "${CERAMIC_NETWORK}",
        "pubsub-topic": "${CERAMIC_NETWORK_TOPIC}"
    },
    "node": {
        "privateSeedUrl": "inplace:ed25519#${CERAMIC_ADMIN_PRIVATE_KEY}"
    },
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
    config_maps.append(&mut config.ipfs.config_maps(info));
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
        cluster_ip: Some("None".to_owned()),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}

/// Describes how a Ceramic peer should behave.
#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CeramicSpec {
    /// Relative weight of the spec compared to others.
    pub weight: Option<i32>,
    /// Name of a config map with a ceramic-init.sh script that runs as an initialization step.
    pub init_config_map: Option<String>,
    /// Image of the ceramic container.
    pub image: Option<String>,
    /// Pull policy for the ceramic container image.
    pub image_pull_policy: Option<String>,
    /// Configuration of the IPFS container
    pub ipfs: Option<IpfsSpec>,
    /// Resource limits for ceramic nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
}

/// Describes how the IPFS node for a peer should behave.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IpfsSpec {
    Rust(RustIpfsSpec),
    Go(GoIpfsSpec),
}

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RustIpfsSpec {
    pub image: Option<String>,
    pub image_pull_policy: Option<String>,
    // Resource limits for ipfs nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
}

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoIpfsSpec {
    pub image: Option<String>,
    pub image_pull_policy: Option<String>,
    // Resource limits for ipfs nodes, applies to both requests and limits.
    pub resource_limits: Option<ResourceLimitsSpec>,
    pub commands: Option<Vec<String>>,
}

pub struct CeramicConfig {
    pub weight: i32,
    pub init_config_map: String,
    pub image: String,
    pub image_pull_policy: String,
    pub ipfs: IpfsConfig,
    pub resource_limits: ResourceLimitsConfig,
}

/// Bundles all relevant config for a ceramic spec.
pub struct CeramicBundle<'a> {
    pub info: CeramicInfo,
    pub config: &'a CeramicConfig,
    pub net_config: &'a NetworkConfig,
    pub datadog: &'a DataDogConfig,
}

/// Unique identifying information about this ceramic spec.
#[derive(Debug)]
pub struct CeramicInfo {
    pub replicas: i32,
    pub stateful_set: String,
    pub service: String,

    suffix: String,
}

impl CeramicInfo {
    pub fn new(suffix: &str, replicas: i32) -> Self {
        Self {
            replicas,
            suffix: suffix.to_owned(),
            stateful_set: format!("ceramic-{suffix}"),
            service: format!("ceramic-{suffix}"),
        }
    }
}

impl CeramicInfo {
    /// Generate a new uninque name for this ceramic spec
    /// Generated name is deterministic for a given input name.
    pub fn new_name(&self, name: &str) -> String {
        format!("{name}-{}", self.suffix)
    }
    /// Determine the IPFS RPC address of a Ceramic peer
    pub fn ipfs_rpc_addr(&self, ns: &str, peer: i32) -> String {
        format!(
            "http://{}-{peer}.{}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_IPFS_PORT}",
            self.stateful_set, self.service
        )
    }
    /// Determine the Ceramic address of a Ceramic peer
    pub fn ceramic_addr(&self, ns: &str, peer: i32) -> String {
        format!(
            "http://{}-{peer}.{}.{ns}.svc.cluster.local:{CERAMIC_SERVICE_API_PORT}",
            self.stateful_set, self.service
        )
    }
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
    fn config_maps(&self, info: &CeramicInfo) -> BTreeMap<String, BTreeMap<String, String>> {
        match self {
            IpfsConfig::Rust(_) => BTreeMap::new(),
            IpfsConfig::Go(config) => config.config_maps(info),
        }
    }
    fn container(&self, info: &CeramicInfo) -> Container {
        match self {
            IpfsConfig::Rust(config) => config.container(),
            IpfsConfig::Go(config) => config.container(info),
        }
    }
    fn volumes(&self, info: &CeramicInfo) -> Vec<Volume> {
        match self {
            IpfsConfig::Rust(_) => Vec::new(),
            IpfsConfig::Go(config) => config.volumes(info),
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
impl From<RustIpfsSpec> for RustIpfsConfig {
    fn from(value: RustIpfsSpec) -> Self {
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
    commands: Vec<String>,
}
impl Default for GoIpfsConfig {
    fn default() -> Self {
        Self {
            image: "ipfs/kubo:v0.19.1@sha256:c4527752a2130f55090be89ade8dde8f8a5328ec72570676b90f66e2cabf827d".to_owned(),
            image_pull_policy: "IfNotPresent".to_owned(),
            resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("512Mi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
            commands: vec![],
        }
    }
}
impl From<GoIpfsSpec> for GoIpfsConfig {
    fn from(value: GoIpfsSpec) -> Self {
        let default = GoIpfsConfig::default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
            commands: value.commands.unwrap_or(default.commands),
        }
    }
}

impl Default for CeramicConfig {
    fn default() -> Self {
        Self {
            weight: 1,
            init_config_map: INIT_CONFIG_MAP_NAME.to_owned(),
            image: "ceramicnetwork/composedb:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
            ipfs: IpfsConfig::default(),
            resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("1Gi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
        }
    }
}

pub struct CeramicConfigs(pub Vec<CeramicConfig>);

impl From<Vec<CeramicSpec>> for CeramicConfigs {
    fn from(value: Vec<CeramicSpec>) -> Self {
        if value.is_empty() {
            Self(vec![CeramicConfig::default()])
        } else {
            Self(value.into_iter().map(CeramicConfig::from).collect())
        }
    }
}

impl From<CeramicSpec> for CeramicConfig {
    fn from(value: CeramicSpec) -> Self {
        let default = Self::default();
        Self {
            weight: value.weight.unwrap_or(default.weight),
            init_config_map: value.init_config_map.unwrap_or(default.init_config_map),
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
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
        match value {
            IpfsSpec::Rust(spec) => Self::Rust(spec.into()),
            IpfsSpec::Go(spec) => Self::Go(spec.into()),
        }
    }
}

impl RustIpfsConfig {
    fn container(&self) -> Container {
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
            image: Some(self.image.to_owned()),
            image_pull_policy: Some(self.image_pull_policy.to_owned()),
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
                    container_port: 9465,
                    name: Some("metrics".to_owned()),
                    protocol: Some("TCP".to_owned()),
                    ..Default::default()
                },
            ]),
            resources: Some(ResourceRequirements {
                limits: Some(self.resource_limits.clone().into()),
                requests: Some(self.resource_limits.clone().into()),
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
    fn config_maps(&self, info: &CeramicInfo) -> BTreeMap<String, BTreeMap<String, String>> {
        let mut ipfs_config = vec![(
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
        )];
        if !self.commands.is_empty() {
            ipfs_config.push((
                "002-config.sh".to_owned(),
                [
                    vec!["#!/bin/sh", "set -ex"],
                    self.commands.iter().map(AsRef::as_ref).collect(),
                ]
                .concat()
                .join("\n"),
            ));
        }
        BTreeMap::from_iter(vec![(
            info.new_name("ipfs-container-init"),
            BTreeMap::from_iter(ipfs_config),
        )])
    }
    fn container(&self, info: &CeramicInfo) -> Container {
        let mut volume_mounts = vec![
            VolumeMount {
                mount_path: "/data/ipfs".to_owned(),
                name: IPFS_DATA_PV_CLAIM.to_owned(),
                ..Default::default()
            },
            VolumeMount {
                mount_path: "/container-init.d/001-config.sh".to_owned(),
                name: info.new_name("ipfs-container-init"),
                // Use an explict subpath otherwise, k8s uses symlinks which breaks
                // kubo's init logic.
                sub_path: Some("001-config.sh".to_owned()),
                ..Default::default()
            },
        ];
        if !self.commands.is_empty() {
            volume_mounts.push(VolumeMount {
                mount_path: "/container-init.d/002-config.sh".to_owned(),
                name: info.new_name("ipfs-container-init"),
                sub_path: Some("002-config.sh".to_owned()),
                ..Default::default()
            })
        }
        Container {
            image: Some(self.image.to_owned()),
            image_pull_policy: Some(self.image_pull_policy.to_owned()),
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
                    container_port: 9465,
                    name: Some("metrics".to_owned()),
                    protocol: Some("TCP".to_owned()),
                    ..Default::default()
                },
            ]),
            resources: Some(ResourceRequirements {
                limits: Some(self.resource_limits.clone().into()),
                requests: Some(self.resource_limits.clone().into()),
                ..Default::default()
            }),
            volume_mounts: Some(volume_mounts),
            ..Default::default()
        }
    }
    fn volumes(&self, info: &CeramicInfo) -> Vec<Volume> {
        vec![Volume {
            name: info.new_name("ipfs-container-init"),
            config_map: Some(ConfigMapVolumeSource {
                default_mode: Some(0o755),
                name: Some(info.new_name("ipfs-container-init")),
                ..Default::default()
            }),
            ..Default::default()
        }]
    }
}

pub fn stateful_set_spec(ns: &str, bundle: &CeramicBundle<'_>) -> StatefulSetSpec {
    let mut ceramic_env = vec![
        EnvVar {
            name: "CERAMIC_NETWORK".to_owned(),
            value: Some(bundle.net_config.network_type.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CERAMIC_NETWORK_TOPIC".to_owned(),
            value: Some(bundle.net_config.pubsub_topic.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "ETH_RPC_URL".to_owned(),
            value: Some(bundle.net_config.eth_rpc_url.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "CAS_API_URL".to_owned(),
            value: Some(bundle.net_config.cas_api_url.to_owned()),
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

    bundle.datadog.inject_env(&mut ceramic_env);

    let mut volumes = vec![
        Volume {
            empty_dir: Some(EmptyDirVolumeSource::default()),
            name: "config-volume".to_owned(),
            ..Default::default()
        },
        Volume {
            config_map: Some(ConfigMapVolumeSource {
                default_mode: Some(0o755),
                name: Some(bundle.config.init_config_map.to_owned()),
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

    volumes.append(&mut bundle.config.ipfs.volumes(&bundle.info));

    StatefulSetSpec {
        pod_management_policy: Some("Parallel".to_owned()),
        replicas: Some(bundle.info.replicas),
        selector: LabelSelector {
            match_labels: selector_labels(CERAMIC_APP),
            ..Default::default()
        },
        service_name: bundle.info.service.clone(),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                annotations: Some(BTreeMap::from_iter(vec![(
                    "prometheus/path".to_owned(),
                    "/metrics".to_owned(),
                )]))
                .map(|mut annotations| {
                    bundle.datadog.inject_annotations(&mut annotations);
                    annotations
                }),

                labels: selector_labels(CERAMIC_APP).map(|mut lbls| {
                    lbls.append(&mut managed_labels().unwrap());
                    bundle
                        .datadog
                        .inject_labels(&mut lbls, ns.to_owned(), "ceramic".to_owned());
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
                        ]),
                        env: Some(ceramic_env),
                        image: Some(bundle.config.image.clone()),
                        image_pull_policy: Some(bundle.config.image_pull_policy.clone()),
                        name: "ceramic".to_owned(),
                        ports: Some(vec![
                            ContainerPort {
                                container_port: CERAMIC_SERVICE_API_PORT,
                                name: Some("api".to_owned()),
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: 9464,
                                name: Some("metrics".to_owned()),
                                protocol: Some("TCP".to_owned()),
                                ..Default::default()
                            },
                        ]),
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
                            limits: Some(bundle.config.resource_limits.clone().into()),
                            requests: Some(bundle.config.resource_limits.clone().into()),
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
                    bundle.config.ipfs.container(&bundle.info),
                ],
                init_containers: Some(vec![Container {
                    command: Some(vec![
                        "/bin/bash".to_owned(),
                        "-c".to_owned(),
                        "/ceramic-init/ceramic-init.sh".to_owned(),
                    ]),
                    env: Some(init_env),
                    image: Some(bundle.config.image.to_owned()),
                    image_pull_policy: Some(bundle.config.image_pull_policy.to_owned()),
                    name: "init-ceramic-config".to_owned(),
                    resources: Some(ResourceRequirements {
                        limits: Some(bundle.config.resource_limits.clone().into()),
                        requests: Some(bundle.config.resource_limits.clone().into()),
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
