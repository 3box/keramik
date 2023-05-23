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

use crate::network::controller::{
    CAS_APP, CAS_IFPS_SERVICE_NAME, CAS_IPFS_APP, CAS_POSTGRES_APP,
    CAS_POSTGRES_SERVICE_NAME, CAS_SERVICE_NAME, GANACHE_APP, GANACHE_SERVICE_NAME,
};

use crate::utils::selector_labels;

// TODO make this a deployment
pub fn cas_stateful_set_spec() -> StatefulSetSpec {
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
                                        format!("http://{CAS_IFPS_SERVICE_NAME}:5001"),
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
                        image: Some(
                            "ceramicnetwork/ceramic-anchor-service".to_owned(),
                        ),
                        image_pull_policy: Some(
                            "Always".to_owned(),
                        ),
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
                                limits: Some(BTreeMap::from_iter(vec![
                                                ("cpu".to_owned(), Quantity(
                                            "250m".to_owned(),
                                        )),
                                        ("ephemeral-storage".to_owned(), Quantity(
                                            "1Gi".to_owned(),
                                        )),
                                        ("memory".to_owned(), Quantity(
                                            "512Mi".to_owned(),
                                        )),
                                    ])),
                                requests: Some(BTreeMap::from_iter(vec![
                                        ("cpu".to_owned(), Quantity(
                                            "250m".to_owned(),
                                        )),
                                        ("ephemeral-storage".to_owned(), Quantity(
                                            "1Gi".to_owned(),
                                        )),
                                        ("memory".to_owned(), Quantity(
                                            "512Mi".to_owned(),
                                        )),
                                    ])),
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
pub fn cas_ipfs_stateful_set_spec() -> StatefulSetSpec {
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(CAS_IPFS_APP),
            ..Default::default()
        },
        service_name: CAS_IFPS_SERVICE_NAME.to_owned(),
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
pub fn ganache_stateful_set_spec() -> StatefulSetSpec {
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
                        limits: Some(BTreeMap::from_iter(vec![
                            ("cpu".to_owned(), Quantity("250m".to_owned())),
                            ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                            ("memory".to_owned(), Quantity("1Gi".to_owned())),
                        ])),
                        requests: Some(BTreeMap::from_iter(vec![
                            ("cpu".to_owned(), Quantity("250m".to_owned())),
                            ("ephemeral-storage".to_owned(), Quantity("1Gi".to_owned())),
                            ("memory".to_owned(), Quantity("1Gi".to_owned())),
                        ])),
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
pub fn postgres_stateful_set_spec() -> StatefulSetSpec {
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
