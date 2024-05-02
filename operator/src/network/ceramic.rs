use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::{RollingUpdateStatefulSetStrategy, StatefulSetSpec, StatefulSetUpdateStrategy},
        core::v1::{
            ConfigMapVolumeSource, Container, ContainerPort, EmptyDirVolumeSource, EnvVar,
            EnvVarSource, HTTPGetAction, PersistentVolumeClaim, PersistentVolumeClaimSpec,
            PersistentVolumeClaimVolumeSource, PodSecurityContext, PodSpec, PodTemplateSpec, Probe,
            ResourceRequirements, SecretKeySelector, SecurityContext, ServicePort, ServiceSpec,
            Volume, VolumeMount,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, util::intstr::IntOrString,
    },
};
use kube::core::ObjectMeta;

use crate::{
    labels::{managed_labels, selector_labels},
    network::{
        controller::{
            CAS_SERVICE_NAME, CERAMIC_APP, CERAMIC_POSTGRES_SECRET_NAME, CERAMIC_SERVICE_API_PORT,
            CERAMIC_SERVICE_IPFS_PORT, GANACHE_SERVICE_NAME, INIT_CONFIG_MAP_NAME,
            NETWORK_DEV_MODE_RESOURCES,
        },
        datadog::DataDogConfig,
        ipfs::{IpfsConfig, IpfsInfo, IPFS_DATA_PV_CLAIM},
        node_affinity::NodeAffinityConfig,
        resource_limits::ResourceLimitsConfig,
        CeramicSpec, NetworkSpec, NetworkType,
    },
    utils::override_and_sort_env_vars,
};

use super::debug_mode_security_context;

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
        "db": "postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@localhost/${POSTGRES_DB}",
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

pub struct CeramicConfig {
    pub weight: i32,
    pub init_config_map: String,
    pub init_image_name: String,
    pub image: String,
    pub image_pull_policy: String,
    pub ipfs: IpfsConfig,
    pub resource_limits: ResourceLimitsConfig,
    pub postgres_resource_limits: ResourceLimitsConfig,
    pub env: Option<BTreeMap<String, String>>,
}

impl CeramicConfig {
    pub fn network_default() -> Self {
        if NETWORK_DEV_MODE_RESOURCES.load(std::sync::atomic::Ordering::Relaxed) {
            Self {
                resource_limits: ResourceLimitsConfig::dev_default(),
                postgres_resource_limits: ResourceLimitsConfig::dev_default(),
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
}

/// Bundles all relevant config for a ceramic spec.
pub struct CeramicBundle<'a> {
    pub info: CeramicInfo,
    pub config: &'a CeramicConfig,
    pub net_config: &'a NetworkConfig,
    pub datadog: &'a DataDogConfig,
}

// Contains top level config for the network
pub struct NetworkConfig {
    pub private_key_secret: Option<String>,
    pub network_type: NetworkType,
    pub eth_rpc_url: String,
    pub cas_api_url: String,
    pub node_affinity_config: NodeAffinityConfig,
    pub debug_mode: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            private_key_secret: None,
            network_type: NetworkType::default(),
            eth_rpc_url: format!("http://{GANACHE_SERVICE_NAME}:8545"),
            cas_api_url: format!("http://{CAS_SERVICE_NAME}:8081"),
            node_affinity_config: NodeAffinityConfig::default(),
            debug_mode: false,
        }
    }
}

impl From<&NetworkSpec> for NetworkConfig {
    fn from(value: &NetworkSpec) -> Self {
        let default = NetworkConfig::default();
        Self {
            private_key_secret: value.private_key_secret.to_owned(),
            network_type: value
                .network_type
                .to_owned()
                .unwrap_or(default.network_type),
            eth_rpc_url: value.eth_rpc_url.to_owned().unwrap_or(default.eth_rpc_url),
            cas_api_url: value.cas_api_url.to_owned().unwrap_or(default.cas_api_url),
            node_affinity_config: value.into(),
            debug_mode: value.debug_mode.unwrap_or(default.debug_mode),
        }
    }
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
    /// Determine the pod name
    pub fn pod_name(&self, peer: i32) -> String {
        format!("{}-{peer}", self.stateful_set)
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

impl From<&CeramicInfo> for IpfsInfo {
    fn from(value: &CeramicInfo) -> Self {
        Self::new(value.suffix.to_owned())
    }
}

impl Default for CeramicConfig {
    fn default() -> Self {
        Self {
            weight: 1,
            init_config_map: INIT_CONFIG_MAP_NAME.to_owned(),
            image: "ceramicnetwork/composedb:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
            init_image_name: "ceramicnetwork/composedb-cli:latest".to_owned(),
            ipfs: IpfsConfig::default(),
            resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("1Gi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            postgres_resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("1Gi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            env: None,
        }
    }
}

pub struct CeramicConfigs(pub Vec<CeramicConfig>);

impl From<Option<Vec<CeramicSpec>>> for CeramicConfigs {
    fn from(value: Option<Vec<CeramicSpec>>) -> Self {
        if let Some(value) = value {
            if value.is_empty() {
                Self(vec![CeramicConfig::network_default()])
            } else {
                Self(value.into_iter().map(CeramicConfig::from).collect())
            }
        } else {
            Self(vec![CeramicConfig::network_default()])
        }
    }
}

impl From<CeramicSpec> for CeramicConfig {
    fn from(value: CeramicSpec) -> Self {
        let default = Self::network_default();
        Self {
            weight: value.weight.unwrap_or(default.weight),
            init_config_map: value.init_config_map.unwrap_or(default.init_config_map),
            init_image_name: value.init_image_name.unwrap_or(default.init_image_name),
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            ipfs: value.ipfs.map(Into::into).unwrap_or(default.ipfs),
            resource_limits: ResourceLimitsConfig::from_spec(
                value.resource_limits,
                default.resource_limits,
            ),
            postgres_resource_limits: ResourceLimitsConfig::from_spec(
                value.postgres_resource_limits,
                default.postgres_resource_limits,
            ),
            env: value.env,
        }
    }
}

pub fn stateful_set_spec(ns: &str, bundle: &CeramicBundle<'_>) -> StatefulSetSpec {
    let mut ceramic_env = vec![
        EnvVar {
            name: "CERAMIC_NETWORK".to_owned(),
            value: Some(bundle.net_config.network_type.name().to_owned()),
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
        EnvVar {
            name: "POSTGRES_DB".to_owned(),
            value: Some("ceramic".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_USER".to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: "username".to_owned(),
                    name: Some(CERAMIC_POSTGRES_SECRET_NAME.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_PASSWORD".to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: "password".to_owned(),
                    name: Some(CERAMIC_POSTGRES_SECRET_NAME.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    // js-ceramic does not allow specifying the pubsub topic for any network types except "inmemory" or "local"
    if bundle.net_config.network_type == NetworkType::InMemory
        || bundle.net_config.network_type == NetworkType::Local
    {
        ceramic_env.push(EnvVar {
            name: "CERAMIC_NETWORK_TOPIC".to_owned(),
            value: Some(bundle.net_config.network_type.topic()),
            ..Default::default()
        })
    }

    // Apply env overrides, if specified.
    override_and_sort_env_vars(&mut ceramic_env, &bundle.config.env);

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
        Volume {
            name: "postgres-data".to_owned(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: "postgres-data".to_owned(),
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
        template: bundle
            .net_config
            .node_affinity_config
            .apply_to_pod_template(PodTemplateSpec {
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
                        bundle.datadog.inject_labels(
                            &mut lbls,
                            ns.to_owned(),
                            "ceramic".to_owned(),
                        );
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
                            security_context: bundle
                                .net_config
                                .debug_mode
                                .then(debug_mode_security_context),
                            ..Default::default()
                        },
                        Container {
                            image: Some("postgres:15-alpine".to_owned()),
                            image_pull_policy: Some("IfNotPresent".to_owned()),
                            name: "postgres".to_owned(),
                            env: Some(vec![
                                EnvVar {
                                    name: "POSTGRES_DB".to_owned(),
                                    value: Some("ceramic".to_owned()),
                                    ..Default::default()
                                },
                                EnvVar {
                                    name: "POSTGRES_PASSWORD".to_owned(),
                                    value_from: Some(EnvVarSource {
                                        secret_key_ref: Some(SecretKeySelector {
                                            key: "password".to_owned(),
                                            name: Some(CERAMIC_POSTGRES_SECRET_NAME.to_string()),
                                            ..Default::default()
                                        }),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                },
                                EnvVar {
                                    name: "POSTGRES_USER".to_owned(),
                                    value_from: Some(EnvVarSource {
                                        secret_key_ref: Some(SecretKeySelector {
                                            key: "username".to_owned(),
                                            name: Some(CERAMIC_POSTGRES_SECRET_NAME.to_string()),
                                            ..Default::default()
                                        }),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                },
                            ]),
                            ports: Some(vec![ContainerPort {
                                container_port: 5432,
                                name: Some("postgres".to_owned()),
                                ..Default::default()
                            }]),
                            resources: Some(ResourceRequirements {
                                limits: Some(bundle.config.postgres_resource_limits.clone().into()),
                                requests: Some(
                                    bundle.config.postgres_resource_limits.clone().into(),
                                ),
                                ..Default::default()
                            }),
                            volume_mounts: Some(vec![VolumeMount {
                                mount_path: "/var/lib/postgresql".to_owned(),
                                name: "postgres-data".to_owned(),
                                sub_path: Some("ceramic_data".to_owned()),
                                ..Default::default()
                            }]),
                            security_context: Some(SecurityContext {
                                run_as_group: Some(70),
                                run_as_user: Some(70),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        bundle
                            .config
                            .ipfs
                            .container(&bundle.info, bundle.net_config),
                    ],
                    init_containers: Some(vec![Container {
                        command: Some(vec![
                            "/bin/bash".to_owned(),
                            "-c".to_owned(),
                            "/ceramic-init/ceramic-init.sh".to_owned(),
                        ]),
                        env: Some(init_env),
                        image: Some(bundle.config.init_image_name.to_owned()),
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
                    security_context: Some(PodSecurityContext {
                        fs_group: Some(70),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            }),
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
                    storage_class_name: bundle.config.ipfs.storage_class_name(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some("postgres-data".to_owned()),
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
