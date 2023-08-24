use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            ConfigMapVolumeSource, Container, ContainerPort, EnvVar, EnvVarSource,
            PersistentVolumeClaim, PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource,
            PodSecurityContext, PodSpec, PodTemplateSpec, ResourceRequirements, SecretKeySelector,
            ServicePort, ServiceSpec, Volume, VolumeMount,
        },
        rbac::v1::{ClusterRole, ClusterRoleBinding, PolicyRule, RoleRef, Subject},
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, apis::meta::v1::ObjectMeta,
        util::intstr::IntOrString,
    },
};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::utils::selector_labels;

use crate::simulation::controller::{OTEL_ACCOUNT, OTEL_CONFIG_MAP_NAME, OTEL_CR};
use crate::simulation::datadog::DataDogConfig;

pub const OTEL_APP: &str = "otel";

pub fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![
            ServicePort {
                name: Some("otlp-receiver".to_owned()),
                port: 4317,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(4317)),
                ..Default::default()
            },
            ServicePort {
                name: Some("prom-metrics".to_owned()),
                port: 9090,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(9090)),
                ..Default::default()
            },
            ServicePort {
                name: Some("self-metrics".to_owned()),
                port: 8888,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(8888)),
                ..Default::default()
            },
        ]),
        selector: selector_labels(OTEL_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}

pub fn stateful_set_spec(datadog: &DataDogConfig) -> StatefulSetSpec {
    StatefulSetSpec {
        replicas: Some(1),
        service_name: OTEL_APP.to_owned(),
        selector: LabelSelector {
            match_labels: selector_labels(OTEL_APP),
            ..Default::default()
        },
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(OTEL_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                service_account_name: Some(OTEL_ACCOUNT.to_owned()),
                security_context: Some(PodSecurityContext {
                    // Explicitly set a filesystem group to allow writing to mounts
                    fs_group: Some(2000),
                    ..Default::default()
                }),
                containers: vec![Container {
                    name: "opentelemetry".to_owned(),
                    image: Some("public.ecr.aws/r5b3e0r5/3box/otelcol".to_owned()),
                    command: Some(vec![
                        "/otelcol-custom".to_owned(),
                        "--config=/config/otel-config.yaml".to_owned(),
                    ]),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: 4317,
                            name: Some("otlp-receiver".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 9090,
                            name: Some("prom-metrics".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 8888,
                            name: Some("self-metrics".to_owned()),
                            ..Default::default()
                        },
                    ]),
                    env: if datadog.enabled{ Some(vec![
                        EnvVar {
                            name: "DD_API_KEY".to_owned(),
                            value_from: Some(EnvVarSource {
                                secret_key_ref: Some(SecretKeySelector {
                                    key: "api-key".to_owned(),
                                    name: Some("datadog-secret".to_owned()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        EnvVar {
                            name: "DD_SITE".to_owned(),
                            value: Some("us3.datadoghq.com".to_owned()),
                            ..Default::default()
                        },
                    ]) } else { None },
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
                    volume_mounts: Some(vec![
                        VolumeMount {
                            mount_path: "/config".to_owned(),
                            name: "config".to_owned(),
                            read_only: Some(true),
                            ..Default::default()
                        },
                        VolumeMount {
                            mount_path: "/data".to_owned(),
                            name: "otel-data".to_owned(),
                            read_only: Some(false),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }],
                volumes: Some(vec![
                    Volume {
                        config_map: Some(ConfigMapVolumeSource {
                            default_mode: Some(0o755),
                            name: Some(OTEL_CONFIG_MAP_NAME.to_owned()),
                            ..Default::default()
                        }),
                        name: "config".to_owned(),
                        ..Default::default()
                    },
                    Volume {
                        name: "otel-data".to_owned(),
                        persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                            claim_name: "otel-data".to_owned(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
        },
        volume_claim_templates: Some(vec![PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some("otel-data".to_owned()),
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

pub fn cluster_role() -> ClusterRole {
    ClusterRole {
        rules: Some(vec![PolicyRule {
            api_groups: Some(vec!["".to_owned()]),
            resources: Some(vec!["pods".to_owned()]),
            verbs: vec!["get".to_owned(), "list".to_owned(), "watch".to_owned()],
            ..Default::default()
        }]),
        ..Default::default()
    }
}

pub fn cluster_role_binding(ns: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        role_ref: RoleRef {
            kind: "ClusterRole".to_owned(),
            name: OTEL_CR.to_owned(),
            api_group: "rbac.authorization.k8s.io".to_owned(),
        },
        subjects: Some(vec![Subject {
            kind: "ServiceAccount".to_owned(),
            name: OTEL_ACCOUNT.to_owned(),
            namespace: Some(ns.to_owned()),
            ..Default::default()
        }]),
        ..Default::default()
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    receivers: BTreeMap<String, Value>,
    processors: BTreeMap<String, Value>,
    exporters: BTreeMap<String, Value>,
    service: BTreeMap<String, Value>,
}

pub fn config_map_data(datadog: &DataDogConfig) -> BTreeMap<String, String> {
    let mut config = Config {
        receivers: BTreeMap::new(),
        processors: BTreeMap::new(),
        exporters: BTreeMap::new(),
        service: BTreeMap::new(),
    };

    let otel_receivers: Value = serde_yaml::from_str::<Value>(
        r#"{
            "protocols": {
                "grpc": {
                    "endpoint": "0.0.0.0:4317"
                },
                "http": {
                    "endpoint": "0.0.0.0:4318"
                }
            }
        }"#,
    )
    .unwrap();

    let prometheus_receivers: Value = serde_yaml::from_slice::<Value>(
        include_bytes!("./prometheus_receivers.json"),
    )
    .unwrap();

    config.receivers.insert("otlp".to_owned(), otel_receivers);
    config
        .receivers
        .insert("prometheus".to_owned(), prometheus_receivers);

    config.processors.insert("batch".to_owned(), Value::Null);

    let datadog_exporter = serde_yaml::from_str::<Value>(
        r#"{
            "api": {
              "key": "${env:DD_API_KEY}",
              "site": "${env:DD_SITE}"
            }
        }"#,
    )
    .unwrap();

    let logging_exporter = serde_yaml::from_str::<Value>(
        r#"{
              "verbosity": "detailed",
              "sampling_initial": 1,
              "sampling_thereafter": 1
        }"#,
    )
    .unwrap();

    let prometheus_exporter = serde_yaml::from_str::<Value>(
        r#"{
              "endpoint": "0.0.0.0:9090"
        }"#,
    )
    .unwrap();

    let parquet_exporter: Value = serde_yaml::from_str::<Value>(
        r#"{
              "path": "/data/"
        }"#,
    )
    .unwrap();

    if datadog.enabled {
        config
            .exporters
            .insert("datadog".to_owned(), datadog_exporter);
    }
    config
        .exporters
        .insert("logging".to_owned(), logging_exporter);
    config
        .exporters
        .insert("prometheus".to_owned(), prometheus_exporter);
    config
        .exporters
        .insert("parquet".to_owned(), parquet_exporter);

    let pipelines_service: Value = serde_yaml::from_str::<Value>(
        r#"{
            "metrics": {
              "receivers": ["otlp"],
              "processors": ["batch"],
              "exporters": ["datadog", "prometheus", "parquet"]
            }
        }"#,
    )
    .unwrap();

    let telemetry_service: Value = serde_yaml::from_str::<Value>(
        r#"{
            "logs": {
                "level": "info"
            },
            "metrics": {
                "level": "detailed",
                "address": "0.0.0.0:8888"
            }
        }"#,
    )
    .unwrap();

    config
        .service
        .insert("pipelines".to_owned(), pipelines_service);
    config
        .service
        .insert("telemetry".to_owned(), telemetry_service);

    let yaml = serde_yaml::to_string(&config).unwrap();

    BTreeMap::from_iter(vec![("otel-config.yaml".to_owned(), yaml)])
}
