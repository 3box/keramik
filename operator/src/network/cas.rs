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

use crate::{
    labels::{managed_labels, selector_labels},
    network::{
        ceramic::NetworkConfig,
        controller::{
            CAS_APP, CAS_IPFS_APP, CAS_IPFS_SERVICE_NAME, CAS_POSTGRES_APP,
            CAS_POSTGRES_SECRET_NAME, CAS_POSTGRES_SERVICE_NAME, CAS_SERVICE_NAME,
            DEFAULT_METRICS_PORT, GANACHE_APP, GANACHE_SERVICE_NAME, LOCALSTACK_APP,
            LOCALSTACK_SERVICE_NAME, NETWORK_DEV_MODE_RESOURCES,
        },
        datadog::DataDogConfig,
        ipfs::{IpfsConfig, IpfsInfo, IPFS_DATA_PV_CLAIM},
        node_affinity::NodeAffinityConfig,
        resource_limits::ResourceLimitsConfig,
        CasApiSpec, CasSpec,
    },
    utils::override_and_sort_env_vars,
};

const CAS_IPFS_INFO_SUFFIX: &str = "cas";

pub struct CasConfig {
    pub image: String,
    pub image_pull_policy: String,
    pub ipfs: IpfsConfig,
    pub cas_resource_limits: ResourceLimitsConfig,
    pub ganache_resource_limits: ResourceLimitsConfig,
    pub postgres_resource_limits: ResourceLimitsConfig,
    pub localstack_resource_limits: ResourceLimitsConfig,
    pub api: CasApiConfig,
}

#[derive(Default)]
pub struct CasApiConfig {
    pub env: Option<BTreeMap<String, String>>,
}

impl From<CasApiSpec> for CasApiConfig {
    fn from(value: CasApiSpec) -> Self {
        Self { env: value.env }
    }
}

impl CasConfig {
    pub fn network_default() -> Self {
        if NETWORK_DEV_MODE_RESOURCES.load(std::sync::atomic::Ordering::Relaxed) {
            Self {
                cas_resource_limits: ResourceLimitsConfig::dev_default(),
                ganache_resource_limits: ResourceLimitsConfig::dev_default(),
                postgres_resource_limits: ResourceLimitsConfig::dev_default(),
                localstack_resource_limits: ResourceLimitsConfig::dev_default(),
                ..Default::default()
            }
        } else {
            Self::default()
        }
    }
}

// Define clear defaults for this config
impl Default for CasConfig {
    fn default() -> Self {
        Self {
            image: "ceramicnetwork/ceramic-anchor-service:latest".to_owned(),
            image_pull_policy: "Always".to_owned(),
            cas_resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("1Gi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            ipfs: Default::default(),
            ganache_resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("1Gi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            postgres_resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("512Mi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            localstack_resource_limits: ResourceLimitsConfig {
                cpu: Some(Quantity("250m".to_owned())),
                memory: Some(Quantity("1Gi".to_owned())),
                storage: Quantity("1Gi".to_owned()),
            },
            api: Default::default(),
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
        let default = Self::network_default();
        Self {
            image: value.image.unwrap_or(default.image),
            image_pull_policy: value.image_pull_policy.unwrap_or(default.image_pull_policy),
            cas_resource_limits: ResourceLimitsConfig::from_spec(
                value.cas_resource_limits,
                default.cas_resource_limits,
            ),
            ipfs: value.ipfs.map(Into::into).unwrap_or(default.ipfs),
            ganache_resource_limits: ResourceLimitsConfig::from_spec(
                value.ganache_resource_limits,
                default.ganache_resource_limits,
            ),
            postgres_resource_limits: ResourceLimitsConfig::from_spec(
                value.postgres_resource_limits,
                default.postgres_resource_limits,
            ),
            localstack_resource_limits: ResourceLimitsConfig::from_spec(
                value.localstack_resource_limits,
                default.localstack_resource_limits,
            ),
            api: value.api.map(Into::into).unwrap_or(default.api),
        }
    }
}

pub fn config_maps(config: impl Into<CasConfig>) -> BTreeMap<String, BTreeMap<String, String>> {
    let config = config.into();
    let ipfs_info = IpfsInfo::new(CAS_IPFS_INFO_SUFFIX.to_string());
    config.ipfs.config_maps(ipfs_info)
}

// TODO make this a deployment
pub fn cas_stateful_set_spec(
    ns: &str,
    config: impl Into<CasConfig>,
    datadog: &DataDogConfig,
    node_affinity_config: &NodeAffinityConfig,
) -> StatefulSetSpec {
    let config = config.into();
    let pg_env = vec![
        EnvVar {
            name: "DB_NAME".to_owned(),
            value: Some("anchor_db".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "DB_HOST".to_owned(),
            value: Some(CAS_POSTGRES_SERVICE_NAME.to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "DB_USERNAME".to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: "username".to_owned(),
                    name: Some(CAS_POSTGRES_SECRET_NAME.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "DB_PASSWORD".to_owned(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    key: "password".to_owned(),
                    name: Some(CAS_POSTGRES_SECRET_NAME.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];
    let aws_env = vec![
        EnvVar {
            name: "AWS_ACCOUNT_ID".to_owned(),
            value: Some("000000000000".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "AWS_REGION".to_owned(),
            value: Some("us-east-2".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "AWS_ACCESS_KEY_ID".to_owned(),
            value: Some(".".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "AWS_SECRET_ACCESS_KEY".to_owned(),
            value: Some(".".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "SQS_QUEUE_URL".to_owned(),
            value: Some("http://localstack:4566/000000000000/cas-anchor-dev-".to_owned()),
            ..Default::default()
        },
    ];
    let eth_env = vec![
        EnvVar {
            name: "ETH_GAS_LIMIT".to_owned(),
            value: Some("4712388".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "ETH_NETWORK".to_owned(),
            value: Some("ganache".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "ETH_RPC_URL".to_owned(),
            value: Some("http://ganache:8545".to_owned()),
            ..Default::default()
        },
        EnvVar {
            name: "ETH_WALLET_PK".to_owned(),
            value: Some(
                "0x06dd0990d19001c57eeea6d32e8fdeee40d3945962caf18c18c3930baa5a6ec9".to_owned(),
            ),
            ..Default::default()
        },
        EnvVar {
            name: "ETH_CONTRACT_ADDRESS".to_owned(),
            value: Some("0x231055A0852D67C7107Ad0d0DFeab60278fE6AdC".to_owned()),
            ..Default::default()
        },
    ];
    let cas_node_env = [
        pg_env.clone(),
        aws_env.clone(),
        eth_env.clone(),
        vec![
            EnvVar {
                name: "NODE_ENV".to_owned(),
                value: Some("dev".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "LOG_LEVEL".to_owned(),
                value: Some("debug".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "MERKLE_CAR_STORAGE_MODE".to_owned(),
                value: Some("s3".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "S3_BUCKET_NAME".to_owned(),
                value: Some("ceramic-dev-cas".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "S3_ENDPOINT".to_owned(),
                value: Some("http://localstack:4566".to_owned()),
                ..Default::default()
            },
        ],
    ]
    .concat();

    let mut cas_api_env = [
        cas_node_env.clone(),
        vec![
            EnvVar {
                name: "APP_MODE".to_owned(),
                value: Some("server".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "APP_PORT".to_owned(),
                value: Some("8081".to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "METRICS_PORT".to_owned(),
                value: Some(DEFAULT_METRICS_PORT.to_string()),
                ..Default::default()
            },
        ],
    ]
    .concat();

    datadog.inject_env(&mut cas_api_env);

    // Apply the CAS API env overrides, if specified.
    override_and_sort_env_vars(&mut cas_api_env, &config.api.env);

    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(CAS_APP),
            ..Default::default()
        },
        service_name: CAS_SERVICE_NAME.to_owned(),
        template: node_affinity_config.apply_to_pod_template(PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(CAS_APP).map(|mut lbls| {
                    lbls.append(&mut managed_labels().unwrap());
                    datadog.inject_labels(&mut lbls, ns.to_owned(), "cas".to_owned());
                    lbls
                }),

                annotations: Some(BTreeMap::new()).map(|mut annotations| {
                    datadog.inject_annotations(&mut annotations);
                    annotations
                }),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                init_containers: Some(vec![
                    Container {
                        env: Some(eth_env),
                        image: Some("public.ecr.aws/r5b3e0r5/3box/cas-contract".to_owned()),
                        image_pull_policy: Some("IfNotPresent".to_owned()),
                        name: "launch-contract".to_owned(),
                        ..Default::default()
                    },
                    Container {
                        env: Some(
                            [
                                pg_env.clone(),
                                vec![EnvVar {
                                    name: "NODE_ENV".to_owned(),
                                    value: Some("dev".to_owned()),
                                    ..Default::default()
                                }],
                            ]
                            .concat(),
                        ),
                        command: Some(
                            ["./node_modules/knex/bin/cli.js", "migrate:latest"]
                                .map(String::from)
                                .to_vec(),
                        ),
                        image: Some(config.image.clone()),
                        image_pull_policy: Some(config.image_pull_policy.clone()),
                        name: "cas-migrations".to_owned(),
                        ..Default::default()
                    },
                ]),
                containers: vec![
                    Container {
                        env: Some(cas_api_env),
                        image: Some(config.image.clone()),
                        image_pull_policy: Some(config.image_pull_policy.clone()),
                        name: "cas-api".to_owned(),
                        ports: Some(vec![ContainerPort {
                            container_port: 8081,
                            ..Default::default()
                        }]),
                        resources: Some(ResourceRequirements {
                            limits: Some(config.cas_resource_limits.clone().into()),
                            requests: Some(config.cas_resource_limits.clone().into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Container {
                        env: Some(
                            [
                                cas_node_env.clone(),
                                vec![
                                    EnvVar {
                                        name: "APP_MODE".to_owned(),
                                        value: Some("continual-anchoring".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "IPFS_API_URL".to_owned(),
                                        value: Some(format!("http://{CAS_IPFS_SERVICE_NAME}:5001")),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "IPFS_API_TIMEOUT".to_owned(),
                                        value: Some("120000".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "IPFS_PUBSUB_TOPIC".to_owned(),
                                        value: Some("/ceramic/local-keramik".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "MERKLE_DEPTH_LIMIT".to_owned(),
                                        value: Some("0".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "USE_SMART_CONTRACT_ANCHORS".to_owned(),
                                        value: Some("true".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "SCHEDULER_STOP_AFTER_NO_OP".to_owned(),
                                        value: Some("false".to_owned()),
                                        ..Default::default()
                                    },
                                ],
                            ]
                            .concat(),
                        ),
                        image: Some(config.image),
                        image_pull_policy: Some(config.image_pull_policy),
                        name: "cas-worker".to_owned(),
                        resources: Some(ResourceRequirements {
                            limits: Some(config.cas_resource_limits.clone().into()),
                            requests: Some(config.cas_resource_limits.clone().into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Container {
                        env: Some(
                            [
                                pg_env,
                                aws_env,
                                vec![
                                    EnvVar {
                                        name: "AWS_ENDPOINT".to_owned(),
                                        value: Some("http://localstack:4566".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "ANCHOR_BATCH_SIZE".to_owned(),
                                        value: Some("20".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "ANCHOR_BATCH_LINGER".to_owned(),
                                        value: Some("10s".to_owned()),
                                        ..Default::default()
                                    },
                                    // Disable worker monitoring since we're not launching workers
                                    EnvVar {
                                        name: "ANCHOR_BATCH_MONITOR_TICK".to_owned(),
                                        value: Some("9223372036854775807ns".to_owned()),
                                        ..Default::default()
                                    },
                                    EnvVar {
                                        name: "POLL_END_CHECKPOINT_DELTA".to_owned(),
                                        value: Some("0s".to_owned()),
                                        ..Default::default()
                                    },
                                    // Don't launch any workers through the scheduler since we're going to use a long-lived
                                    // worker.
                                    EnvVar {
                                        name: "MAX_ANCHOR_WORKERS".to_owned(),
                                        value: Some("0".to_owned()),
                                        ..Default::default()
                                    },
                                ],
                            ]
                            .concat(),
                        ),
                        image: Some("public.ecr.aws/r5b3e0r5/3box/go-cas:latest".to_owned()),
                        name: "cas-scheduler".to_owned(),
                        resources: Some(ResourceRequirements {
                            limits: Some(config.cas_resource_limits.clone().into()),
                            requests: Some(config.cas_resource_limits.into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ],
                volumes: Some(vec![Volume {
                    name: "cas-data".to_owned(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: "cas-data".to_owned(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        }),
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("cas-data".to_owned()),
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

pub fn cas_ipfs_stateful_set_spec(
    config: impl Into<CasConfig>,
    net_config: &NetworkConfig,
) -> StatefulSetSpec {
    let config = config.into();

    let mut volumes = vec![Volume {
        name: IPFS_DATA_PV_CLAIM.to_owned(),
        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
            claim_name: IPFS_DATA_PV_CLAIM.to_owned(),
            ..Default::default()
        }),
        ..Default::default()
    }];

    let ipfs_info = IpfsInfo::new(CAS_IPFS_INFO_SUFFIX.to_string());
    volumes.append(&mut config.ipfs.volumes(ipfs_info.clone()));

    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(CAS_IPFS_APP),
            ..Default::default()
        },
        service_name: CAS_IPFS_SERVICE_NAME.to_owned(),
        template: net_config
            .node_affinity_config
            .apply_to_pod_template(PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: selector_labels(CAS_IPFS_APP),
                    annotations: Some(BTreeMap::from_iter(vec![(
                        "prometheus/path".to_owned(),
                        "/metrics".to_owned(),
                    )])),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![config.ipfs.container(ipfs_info, net_config)],
                    volumes: Some(volumes),
                    ..Default::default()
                }),
            }),
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
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
        cluster_ip: Some("None".to_owned()),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}
pub fn ganache_stateful_set_spec(
    config: impl Into<CasConfig>,
    node_affinity_config: &NodeAffinityConfig,
) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(GANACHE_APP),
            ..Default::default()
        },
        service_name: GANACHE_SERVICE_NAME.to_owned(),
        template: node_affinity_config.apply_to_pod_template(PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(GANACHE_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    command: Some([
                        "node",
                        "/app/dist/node/cli.js",
                        "--miner.blockTime=5",
                        "--mnemonic='move sense much taxi wave hurry recall stairs thank brother nut woman'",
                        "--networkId=5777",
                        "-l=80000000",
                        "--quiet",
                    ].map(String::from).to_vec()),
                    image: Some("trufflesuite/ganache".to_owned()),
                    image_pull_policy: Some("IfNotPresent".to_owned()),
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
        }),
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
pub fn postgres_stateful_set_spec(
    config: impl Into<CasConfig>,
    node_affinity_config: &NodeAffinityConfig,
) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(CAS_POSTGRES_APP),
            ..Default::default()
        },
        service_name: CAS_POSTGRES_SERVICE_NAME.to_owned(),
        template: node_affinity_config.apply_to_pod_template(PodTemplateSpec {
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
                                    name: Some(CAS_POSTGRES_SECRET_NAME.to_string()),
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
                                    name: Some(CAS_POSTGRES_SECRET_NAME.to_string()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    image: Some("postgres:15-alpine".to_owned()),
                    image_pull_policy: Some("IfNotPresent".to_owned()),
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
                        sub_path: Some("ceramic_data".to_owned()),
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
        }),
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

pub fn localstack_stateful_set_spec(
    config: impl Into<CasConfig>,
    node_affinity_config: &NodeAffinityConfig,
) -> StatefulSetSpec {
    let config = config.into();
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(LOCALSTACK_APP),
            ..Default::default()
        },
        service_name: LOCALSTACK_SERVICE_NAME.to_owned(),
        template: node_affinity_config.apply_to_pod_template(PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(LOCALSTACK_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    image: Some("gresau/localstack-persist:3".to_owned()),
                    image_pull_policy: Some("IfNotPresent".to_owned()),
                    name: "localstack".to_owned(),
                    ports: Some(vec![ContainerPort {
                        container_port: 4566,
                        ..Default::default()
                    }]),
                    resources: Some(ResourceRequirements {
                        limits: Some(config.localstack_resource_limits.clone().into()),
                        requests: Some(config.localstack_resource_limits.into()),
                        ..Default::default()
                    }),
                    volume_mounts: Some(vec![VolumeMount {
                        mount_path: "/persisted-data".to_owned(),
                        name: "localstack-data".to_owned(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                volumes: Some(vec![Volume {
                    name: "localstack-data".to_owned(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: "localstack-data".to_owned(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        }),
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("localstack-data".to_owned()),
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

pub fn localstack_service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("localstack".to_owned()),
            port: 4566,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(4566)),
            ..Default::default()
        }]),
        selector: selector_labels(LOCALSTACK_APP),
        type_: Some("NodePort".to_owned()),
        ..Default::default()
    }
}
