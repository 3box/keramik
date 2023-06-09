use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            Container, ContainerPort, EnvVar, EnvVarSource, PersistentVolumeClaim,
            PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, PodSecurityContext,
            PodSpec, PodTemplateSpec, ResourceRequirements, SecretKeySelector, ServicePort,
            ServiceSpec, Volume, VolumeMount,
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
        CAS_APP, CAS_IPFS_APP, CAS_IPFS_SERVICE_NAME, CAS_POSTGRES_APP, CAS_POSTGRES_SERVICE_NAME,
        CAS_SERVICE_NAME, GANACHE_APP, GANACHE_SERVICE_NAME,
    },
    utils::{ResourceLimitsConfig, ResourceLimitsSpec},
};

use crate::utils::selector_labels;

/// Defines details about how CAS is deployed
#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CasSpec {
    /// Image of the runner for the bootstrap job.
    pub image: Option<String>,
    /// Image pull policy for the bootstrap job.
    pub image_pull_policy: Option<String>,
    /// Resource limits for the CAS pod, applies to both requests and limits.
    pub cas_resource_limits: Option<ResourceLimitsSpec>,
    /// Resource limits for the CAS IPFS pod, applies to both requests and limits.
    pub ipfs_resource_limits: Option<ResourceLimitsSpec>,
    /// Resource limits for the Ganache pod, applies to both requests and limits.
    pub ganache_resource_limits: Option<ResourceLimitsSpec>,
    /// Resource limits for the CAS Postgres pod, applies to both requests and limits.
    pub postgres_resource_limits: Option<ResourceLimitsSpec>,
}

pub struct CasConfig {
    pub image: String,
    pub image_pull_policy: String,
    pub cas_resource_limits: ResourceLimitsConfig,
    pub ipfs_resource_limits: ResourceLimitsConfig,
    pub ganache_resource_limits: ResourceLimitsConfig,
    pub postgres_resource_limits: ResourceLimitsConfig,
}

// Define clear defaults for this config
impl Default for CasConfig {
    fn default() -> Self {
        Self {
            image: "ceramicnetwork/ceramic-anchor-service:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
            cas_resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("1Gi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
            ipfs_resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("512Mi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
            ganache_resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("1Gi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
            postgres_resource_limits: ResourceLimitsConfig {
                cpu: Quantity("250m".to_owned()),
                memory: Quantity("512Mi".to_owned()),
                storage: Quantity("1Gi".to_owned()),
            },
        }
    }
}

impl From<Option<CasSpec>> for CasConfig {
    fn from(value: Option<CasSpec>) -> Self {
        match value {
            Some(spec) => spec.into(),
            None => Default::default(),
        }
    }
}

impl From<CasSpec> for CasConfig {
    fn from(value: CasSpec) -> Self {
        let default = Self::default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            cas_resource_limits: ResourceLimitsConfig::from_spec(
                value.cas_resource_limits,
                default.cas_resource_limits,
            ),
            ipfs_resource_limits: ResourceLimitsConfig::from_spec(
                value.ipfs_resource_limits,
                default.ipfs_resource_limits,
            ),
            ganache_resource_limits: ResourceLimitsConfig::from_spec(
                value.ganache_resource_limits,
                default.ganache_resource_limits,
            ),
            postgres_resource_limits: ResourceLimitsConfig::from_spec(
                value.postgres_resource_limits,
                default.postgres_resource_limits,
            ),
        }
    }
}

// TODO make this a deployment
pub fn cas_stateful_set_spec(config: impl Into<CasConfig>) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
    replicas: Some( 1,),
    selector: LabelSelector {
        match_labels: selector_labels(CAS_APP),
    ..Default::default()},
    service_name: CAS_SERVICE_NAME.to_owned(),
    template: PodTemplateSpec {
        metadata: Some(
            ObjectMeta {
                labels:selector_labels(CAS_APP),
            ..Default::default()},
        ),
        spec: Some(
            PodSpec {
                containers: vec![
                    Container {
                        env: Some(
                            vec![
                                EnvVar {
                                    name: "NODE_ENV".to_owned(),
                                    value: Some(
                                        "dev".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "ANCHOR_EXPIRATION_PERIOD".to_owned(),
                                    value: Some(
                                        "300000".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "ANCHOR_SCHEDULE_EXPRESSION".to_owned(),
                                    value: Some(
                                        "0/1 * * * ? *".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "APP_MODE".to_owned(),
                                    value: Some(
                                        "bundled".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "APP_PORT".to_owned(),
                                    value: Some(
                                        "8081".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "BLOCKCHAIN_CONNECTOR".to_owned(),
                                    value: Some(
                                        "ethereum".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "ETH_NETWORK".to_owned(),
                                    value: Some(
                                        "ganache".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "ETH_RPC_URL".to_owned(),
                                    value: Some(
                                        "http://ganache:8545".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "ETH_WALLET_PK".to_owned(),
                                    value: Some(
                                        "0x16dd0990d19001c50eeea6d32e8fdeef40d3945962caf18c18c3930baa5a6ec9".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "ETH_CONTRACT_ADDRESS".to_owned(),
                                    value: Some(
                                        "0xD3f84Cf6Be3DD0EB16dC89c972f7a27B441A39f2".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "IPFS_API_URL".to_owned(),
                                    value: Some(
                                        format!("http://{CAS_IPFS_SERVICE_NAME}:5001"),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "IPFS_PUBSUB_TOPIC".to_owned(),
                                    value: Some(
                                        "local".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "LOG_LEVEL".to_owned(),
                                    value: Some(
                                        "debug".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "MERKLE_DEPTH_LIMIT".to_owned(),
                                    value: Some(
                                        "0".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "VALIDATE_RECORDS".to_owned(),
                                    value: Some(
                                        "false".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "DB_NAME".to_owned(),
                                    value: Some(
                                        "anchor_db".to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "DB_HOST".to_owned(),
                                    value: Some(
                                        CAS_POSTGRES_SERVICE_NAME.to_owned(),
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "DB_USERNAME".to_owned(),
                                    value_from: Some(
                                        EnvVarSource {
                                            secret_key_ref: Some(
                                                SecretKeySelector {
                                                    key: "username".to_owned(),
                                                    name: Some(
                                                        "postgres-auth".to_owned(),
                                                    ),
                                                ..Default::default()},
                                            ),
                                        ..Default::default()},
                                    ),
                                ..Default::default()},
                                EnvVar {
                                    name: "DB_PASSWORD".to_owned(),
                                    value_from: Some(
                                        EnvVarSource {
                                            secret_key_ref: Some(
                                                SecretKeySelector {
                                                    key: "password".to_owned(),
                                                    name: Some(
                                                        "postgres-auth".to_owned(),
                                                    ),
                                                ..Default::default()},
                                            ),
                                        ..Default::default()},
                                    ),
                                ..Default::default()},
                            ],
                        ),
                        image: Some(config.image),
                        image_pull_policy: Some( config.image_pull_policy,),
                        name: "cas".to_owned(),
                        ports: Some(
                            vec![
                                ContainerPort {
                                    container_port: 8081,
                                ..Default::default()},
                            ],
                        ),
                        resources: Some(
                            ResourceRequirements {
                                limits: Some(config.cas_resource_limits.clone().into()),
                                requests: Some(config.cas_resource_limits.into()),
                            ..Default::default()},
                        ),
                        volume_mounts: Some(
                            vec![
                                VolumeMount {
                                    mount_path: "/cas/db".to_owned(),
                                    name: "cas-data".to_owned(),
                                ..Default::default()},
                            ],
                        ),
                    ..Default::default()},
                ],
                volumes: Some(
                    vec![
                        Volume {
                            name: "cas-data".to_owned(),
                            persistent_volume_claim: Some(
                                PersistentVolumeClaimVolumeSource {
                                    claim_name: "cas-data".to_owned(),
                                ..Default::default()},
                            ),
                        ..Default::default()},
                    ],
                ),
            ..Default::default()},
        ),
    },
    volume_claim_templates: Some(
        vec![
            PersistentVolumeClaim {
                metadata: ObjectMeta {
                    name: Some(
                        "cas-data".to_owned(),
                    ),
                ..Default::default()},
                spec: Some(
                    PersistentVolumeClaimSpec {
                        access_modes: Some(
                            vec![
                                "ReadWriteOnce".to_owned(),
                            ],
                        ),
                        resources: Some(
                            ResourceRequirements {
                                requests: Some(BTreeMap::from_iter(vec![
                                                  ("storage".to_owned(), Quantity(
                                            "10Gi".to_owned(),
                                        )),
                                    ])),
                            ..Default::default()},
                        ),
                    ..Default::default()},
                ),
            ..Default::default()},
        ],
    ),
..Default::default()}
}
pub fn cas_service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("cas".to_owned()),
            port: 8081,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(8081)),
            ..Default::default()
        }]),
        selector: selector_labels(CAS_APP),
        type_: Some("NodePort".to_owned()),
        ..Default::default()
    }
}

pub fn cas_ipfs_stateful_set_spec(config: impl Into<CasConfig>) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(CAS_IPFS_APP),
            ..Default::default()
        },
        service_name: CAS_IPFS_SERVICE_NAME.to_owned(),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(CAS_IPFS_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    command: Some(vec![
                        "/usr/bin/ceramic-one".to_owned(),
                        "daemon".to_owned(),
                        "--store-dir".to_owned(),
                        "/data/ipfs".to_owned(),
                        "-b".to_owned(),
                        "0.0.0.0:5001".to_owned(),
                    ]),
                    image: Some("public.ecr.aws/r5b3e0r5/3box/ceramic-one".to_owned()),
                    image_pull_policy: Some("Always".to_owned()),
                    name: "ipfs".to_owned(),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: 4001,
                            name: Some("swarm".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 5001,
                            name: Some("api".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 8080,
                            name: Some("gateway".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 9090,
                            name: Some("metrics".to_owned()),
                            ..Default::default()
                        },
                    ]),
                    resources: Some(ResourceRequirements {
                        limits: Some(config.ipfs_resource_limits.clone().into()),
                        requests: Some(config.ipfs_resource_limits.into()),
                        ..Default::default()
                    }),
                    volume_mounts: Some(vec![VolumeMount {
                        mount_path: "/data/ipfs".to_owned(),
                        name: "cas-ipfs-data".to_owned(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                volumes: Some(vec![Volume {
                    name: "cas-ipfs-data".to_owned(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: "cas-ipfs-data".to_owned(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        },
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("cas-ipfs-data".to_owned()),
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
        }]),
        ..Default::default()
    }
}
pub fn cas_ipfs_service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("cas-ipfs".to_owned()),
            port: 5001,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(5001)),
            ..Default::default()
        }]),
        selector: selector_labels(CAS_IPFS_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}
pub fn ganache_stateful_set_spec(config: impl Into<CasConfig>) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(GANACHE_APP),
            ..Default::default()
        },
        service_name: GANACHE_SERVICE_NAME.to_owned(),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(GANACHE_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    command: Some(vec![
                        "node".to_owned(),
                        "/app/ganache-core.docker.cli.js".to_owned(),
                        "--deterministic".to_owned(),
                        "--db=/ganache/db".to_owned(),
                        "--mnemonic".to_owned(),
                        "move sense much taxi wave hurry recall stairs thank brother nut woman"
                            .to_owned(),
                        "--networkId".to_owned(),
                        "5777".to_owned(),
                        "--hostname".to_owned(),
                        "0.0.0.0".to_owned(),
                        "-l".to_owned(),
                        "80000000".to_owned(),
                        "--quiet".to_owned(),
                    ]),
                    image: Some("trufflesuite/ganache-cli".to_owned()),
                    image_pull_policy: Some("Always".to_owned()),
                    name: "ganache".to_owned(),
                    ports: Some(vec![ContainerPort {
                        container_port: 8545,
                        ..Default::default()
                    }]),
                    resources: Some(ResourceRequirements {
                        limits: Some(config.ganache_resource_limits.clone().into()),
                        requests: Some(config.ganache_resource_limits.into()),
                        ..Default::default()
                    }),
                    volume_mounts: Some(vec![VolumeMount {
                        mount_path: "/ganache-data".to_owned(),
                        name: "ganache-data".to_owned(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                volumes: Some(vec![Volume {
                    name: "ganache-data".to_owned(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: "ganache-data".to_owned(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        },
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("ganache-data".to_owned()),
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
        }]),
        ..Default::default()
    }
}
pub fn ganache_service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("ganache".to_owned()),
            port: 8545,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(8545)),
            ..Default::default()
        }]),
        selector: selector_labels(GANACHE_APP),
        type_: Some("NodePort".to_owned()),
        ..Default::default()
    }
}
pub fn postgres_stateful_set_spec(config: impl Into<CasConfig>) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(CAS_POSTGRES_APP),
            ..Default::default()
        },
        service_name: CAS_POSTGRES_SERVICE_NAME.to_owned(),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(CAS_POSTGRES_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    env: Some(vec![
                        EnvVar {
                            name: "POSTGRES_DB".to_owned(),
                            value: Some("anchor_db".to_owned()),
                            ..Default::default()
                        },
                        EnvVar {
                            name: "POSTGRES_PASSWORD".to_owned(),
                            value_from: Some(EnvVarSource {
                                secret_key_ref: Some(SecretKeySelector {
                                    key: "password".to_owned(),
                                    name: Some("postgres-auth".to_owned()),
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
                                    name: Some("postgres-auth".to_owned()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    image: Some("postgres:15-alpine".to_owned()),
                    image_pull_policy: Some("Always".to_owned()),
                    name: "postgres".to_owned(),
                    ports: Some(vec![ContainerPort {
                        container_port: 5432,
                        name: Some("postgres".to_owned()),
                        ..Default::default()
                    }]),
                    resources: Some(ResourceRequirements {
                        limits: Some(config.postgres_resource_limits.clone().into()),
                        requests: Some(config.postgres_resource_limits.into()),
                        ..Default::default()
                    }),
                    volume_mounts: Some(vec![VolumeMount {
                        mount_path: "/var/lib/postgresql".to_owned(),
                        name: "postgres-data".to_owned(),
                        sub_path: Some("ceradmic_data".to_owned()),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                security_context: Some(PodSecurityContext {
                    fs_group: Some(70),
                    run_as_group: Some(70),
                    run_as_user: Some(70),
                    ..Default::default()
                }),
                volumes: Some(vec![Volume {
                    name: "postgres-data".to_owned(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: "postgres-data".to_owned(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        },
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
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
        }]),
        ..Default::default()
    }
}
pub fn postgres_service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("postgres".to_owned()),
            port: 5432,
            target_port: Some(IntOrString::Int(5432)),
            ..Default::default()
        }]),
        selector: selector_labels(CAS_POSTGRES_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}
