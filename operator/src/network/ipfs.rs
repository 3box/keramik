use std::collections::BTreeMap;

use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, ContainerPort, EnvVar, ResourceRequirements, Volume,
        VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
};

const IPFS_CONTAINER_NAME: &str = "ipfs";
pub const IPFS_DATA_PV_CLAIM: &str = "ipfs-data";
const IPFS_SERVICE_PORT: i32 = 5001;

use crate::network::{
    ceramic::NetworkConfig, controller::NETWORK_DEV_MODE_RESOURCES,
    resource_limits::ResourceLimitsConfig, GoIpfsSpec, IpfsSpec, RustIpfsSpec, NETWORK_LOCAL_ID,
};

use super::debug_mode_security_context;

/// Unique identifying information about this IPFS spec.
#[derive(Debug, Clone)]
pub struct IpfsInfo {
    suffix: String,
}

impl IpfsInfo {
    pub fn new(suffix: String) -> Self {
        Self { suffix }
    }
    /// Generate a new uninque name for this ceramic spec
    /// Generated name is deterministic for a given input name.
    pub fn new_name(&self, name: &str) -> String {
        format!("{name}-{}", self.suffix)
    }
}

pub enum IpfsConfig {
    Rust(RustIpfsConfig),
    Go(GoIpfsConfig),
}
impl From<IpfsSpec> for IpfsConfig {
    fn from(value: IpfsSpec) -> Self {
        match value {
            IpfsSpec::Rust(spec) => Self::Rust(spec.into()),
            IpfsSpec::Go(spec) => Self::Go(spec.into()),
        }
    }
}

impl Default for IpfsConfig {
    fn default() -> Self {
        Self::Rust(Default::default())
    }
}
impl IpfsConfig {
    pub fn config_maps(
        &self,
        info: impl Into<IpfsInfo>,
    ) -> BTreeMap<String, BTreeMap<String, String>> {
        let info = info.into();
        match self {
            IpfsConfig::Rust(_) => BTreeMap::new(),
            IpfsConfig::Go(config) => config.config_maps(&info),
        }
    }
    pub fn container(&self, info: impl Into<IpfsInfo>, net_config: &NetworkConfig) -> Container {
        let info = info.into();
        match self {
            IpfsConfig::Rust(config) => config.container(net_config),
            IpfsConfig::Go(config) => config.container(&info),
        }
    }
    pub fn volumes(&self, info: impl Into<IpfsInfo>) -> Vec<Volume> {
        let info = info.into();
        match self {
            IpfsConfig::Rust(_) => Vec::new(),
            IpfsConfig::Go(config) => config.volumes(&info),
        }
    }
    pub fn storage_class_name(&self) -> Option<String> {
        match self {
            IpfsConfig::Rust(r) => r.storage_class.clone(),
            IpfsConfig::Go(g) => g.storage_class.clone(),
        }
    }
}

pub struct RustIpfsConfig {
    image: String,
    image_pull_policy: String,
    resource_limits: ResourceLimitsConfig,
    storage_class: Option<String>,
    rust_log: String,
    env: Option<BTreeMap<String, String>>,
}

impl RustIpfsConfig {
    pub fn network_default() -> Self {
        if NETWORK_DEV_MODE_RESOURCES.load(std::sync::atomic::Ordering::Relaxed) {
            Self {
                resource_limits: ResourceLimitsConfig::dev_default(),
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
}

impl Default for RustIpfsConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/ceramic-one:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
            resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("512Mi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            storage_class: None,
            rust_log: "info,ceramic_one=debug,multipart=error".to_owned(),
            env: None,
        }
    }
}
impl From<RustIpfsSpec> for RustIpfsConfig {
    fn from(value: RustIpfsSpec) -> Self {
        let default = RustIpfsConfig::network_default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
            rust_log: value.rust_log.unwrap_or(default.rust_log),
            storage_class: value.storage_class,
            env: value.env,
        }
    }
}
impl RustIpfsConfig {
    fn container(&self, net_config: &NetworkConfig) -> Container {
        let mut env = vec![
            EnvVar {
                name: "RUST_LOG".to_owned(),
                value: Some(self.rust_log.to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "CERAMIC_ONE_BIND_ADDRESS".to_owned(),
                value: Some(format!("0.0.0.0:{IPFS_SERVICE_PORT}")),
                ..Default::default()
            },
            EnvVar {
                name: "CERAMIC_ONE_METRICS_BIND_ADDRESS".to_owned(),
                value: Some("0.0.0.0:9465".to_owned()),
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
            EnvVar {
                name: "CERAMIC_ONE_NETWORK".to_owned(),
                value: Some(net_config.network_type.name().to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "CERAMIC_ONE_LOCAL_NETWORK_ID".to_owned(),
                value: Some(NETWORK_LOCAL_ID.to_string()),
                ..Default::default()
            },
            EnvVar {
                name: "CERAMIC_ONE_KADEMLIA_REPLICATION".to_owned(),
                value: Some("6".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "CERAMIC_ONE_KADEMLIA_PARALLELISM".to_owned(),
                value: Some("1".to_owned()),
                ..Default::default()
            },
        ];
        if net_config.debug_mode {
            env.push(EnvVar {
                name: "CERAMIC_ONE_TOKIO_CONSOLE".to_owned(),
                value: Some("true".to_owned()),
                ..Default::default()
            })
        }
        if let Some(extra_env) = &self.env {
            extra_env.iter().for_each(|(key, value)| {
                if let Some((pos, _)) = env.iter().enumerate().find(|(_, var)| &var.name == key) {
                    env.swap_remove(pos);
                }
                env.push(EnvVar {
                    name: key.to_string(),
                    value: Some(value.to_string()),
                    ..Default::default()
                })
            });
        }
        // Sort env vars so we can have stable tests
        env.sort_unstable_by(|a, b| a.name.cmp(&b.name));

        // Construct the set of ports
        let mut ports = vec![
            ContainerPort {
                container_port: 4001,
                name: Some("swarm-tcp".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            },
            ContainerPort {
                container_port: IPFS_SERVICE_PORT,
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
        ];
        if net_config.debug_mode {
            ports.push(ContainerPort {
                container_port: 6669,
                name: Some("tokio-console".to_owned()),
                protocol: Some("TCP".to_owned()),
                ..Default::default()
            })
        }
        Container {
            env: Some(env),
            image: Some(self.image.to_owned()),
            image_pull_policy: Some(self.image_pull_policy.to_owned()),
            name: IPFS_CONTAINER_NAME.to_owned(),
            ports: Some(ports),
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
            security_context: net_config.debug_mode.then(debug_mode_security_context),
            ..Default::default()
        }
    }
}

pub struct GoIpfsConfig {
    image: String,
    image_pull_policy: String,
    resource_limits: ResourceLimitsConfig,
    storage_class: Option<String>,
    commands: Vec<String>,
}

impl GoIpfsConfig {
    pub fn network_default() -> Self {
        if NETWORK_DEV_MODE_RESOURCES.load(std::sync::atomic::Ordering::Relaxed) {
            Self {
                resource_limits: ResourceLimitsConfig::dev_default(),
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
}

impl Default for GoIpfsConfig {
    fn default() -> Self {
        Self {
            image: "ipfs/kubo:v0.19.1@sha256:c4527752a2130f55090be89ade8dde8f8a5328ec72570676b90f66e2cabf827d".to_owned(),
            image_pull_policy: "IfNotPresent".to_owned(),
            resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("512Mi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            storage_class: None,
            commands: vec![],
        }
    }
}
impl From<GoIpfsSpec> for GoIpfsConfig {
    fn from(value: GoIpfsSpec) -> Self {
        let default = GoIpfsConfig::network_default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
            storage_class: value.storage_class,
            commands: value.commands.unwrap_or(default.commands),
        }
    }
}
impl GoIpfsConfig {
    fn config_maps(&self, info: &IpfsInfo) -> BTreeMap<String, BTreeMap<String, String>> {
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
    fn container(&self, info: &IpfsInfo) -> Container {
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
                    container_port: IPFS_SERVICE_PORT,
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
    fn volumes(&self, info: &IpfsInfo) -> Vec<Volume> {
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
