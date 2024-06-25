use std::collections::BTreeMap;

use k8s_openapi::{
    api::core::v1::{
        ConfigMapVolumeSource, Container, ContainerPort, EnvVar, ResourceRequirements, Volume,
        VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
};

const IPFS_CONTAINER_NAME: &str = "ipfs";
const IPFS_STORE_DIR: &str = "/data/ipfs";
pub const IPFS_DATA_PV_CLAIM: &str = "ipfs-data";
const IPFS_SERVICE_PORT: i32 = 5001;

use crate::{
    network::{
        ceramic::NetworkConfig, controller::NETWORK_DEV_MODE_RESOURCES,
        resource_limits::ResourceLimitsConfig, storage::PersistentStorageConfig, GoIpfsSpec,
        IpfsSpec, RustIpfsSpec, NETWORK_LOCAL_ID,
    },
    utils::override_and_sort_env_vars,
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
    pub fn init_container(&self, net_config: &NetworkConfig) -> Option<Container> {
        match self {
            IpfsConfig::Rust(config) => config.init_container(net_config),
            _ => None,
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
    pub fn storage_config(&self) -> &PersistentStorageConfig {
        match self {
            IpfsConfig::Rust(r) => &r.storage,
            IpfsConfig::Go(g) => &g.storage,
        }
    }
}

pub struct RustIpfsConfig {
    image: String,
    image_pull_policy: String,
    resource_limits: ResourceLimitsConfig,
    storage: PersistentStorageConfig,
    rust_log: String,
    env: Option<BTreeMap<String, String>>,
    migration_cmd: Option<Vec<String>>,
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
                cpu: Some(Quantity("1".to_owned())),
                memory: Some(Quantity("1Gi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            storage: PersistentStorageConfig {
                size: Quantity("10Gi".to_owned()),
                class: None,
            },
            rust_log: "info,ceramic_one=debug,multipart=error".to_owned(),
            env: None,
            migration_cmd: None,
        }
    }
}
impl From<RustIpfsSpec> for RustIpfsConfig {
    fn from(value: RustIpfsSpec) -> Self {
        let mut default = RustIpfsConfig::network_default();
        // we prefer the value from PersistentStorageConfig but will fall back to this if provided
        if let Some(class) = value.storage_class {
            default.storage.class = Some(class);
        }
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
            storage: PersistentStorageConfig::from_spec(value.storage, default.storage),
            rust_log: value.rust_log.unwrap_or(default.rust_log),
            env: value.env,
            migration_cmd: value.migration_cmd,
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
                value: Some(IPFS_STORE_DIR.to_owned()),
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

        // Apply env overrides, if specified.
        override_and_sort_env_vars(&mut env, &self.env);

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
                mount_path: IPFS_STORE_DIR.to_owned(),
                name: IPFS_DATA_PV_CLAIM.to_owned(),
                ..Default::default()
            }]),
            security_context: net_config.debug_mode.then(debug_mode_security_context),
            ..Default::default()
        }
    }

    fn init_container(&self, net_config: &NetworkConfig) -> Option<Container> {
        self.migration_cmd.as_ref().map(|cmd| Container {
            name: "ipfs-migration".to_string(),
            command: Some(
                vec!["/usr/bin/ceramic-one", "migrations"]
                    .into_iter()
                    .chain(cmd.iter().map(String::as_str))
                    .map(ToOwned::to_owned)
                    .collect(),
            ),
            ..self.container(net_config)
        })
    }
}

pub struct GoIpfsConfig {
    image: String,
    image_pull_policy: String,
    resource_limits: ResourceLimitsConfig,
    storage: PersistentStorageConfig,
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
            storage: PersistentStorageConfig {
                size: Quantity("10Gi".to_owned()),
                class: None,
            },
            commands: vec![],
        }
    }
}
impl From<GoIpfsSpec> for GoIpfsConfig {
    fn from(value: GoIpfsSpec) -> Self {
        let mut default = GoIpfsConfig::network_default();
        // we prefer the value from PersistentStorageConfig but will fall back to this if provided
        if let Some(class) = value.storage_class {
            default.storage.class = Some(class);
        }
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
            storage: PersistentStorageConfig::from_spec(value.storage, default.storage),
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
                mount_path: IPFS_STORE_DIR.to_owned(),
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
