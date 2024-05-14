use std::{collections::BTreeMap, sync::Arc};

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            ConfigMapVolumeSource, Container, ContainerPort, PersistentVolumeClaim,
            PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, PodSecurityContext,
            PodSpec, PodTemplateSpec, ResourceRequirements, ServicePort, ServiceSpec, Volume,
            VolumeMount,
        },
        rbac::v1::{ClusterRole, ClusterRoleBinding, PolicyRule, RoleRef, Subject},
    },
    apimachinery::pkg::{
        api::resource::Quantity,
        apis::meta::v1::LabelSelector,
        apis::meta::v1::{ObjectMeta, OwnerReference},
        util::intstr::IntOrString,
    },
};
use rand::RngCore;

use crate::{
    labels::selector_labels,
    network::{
        controller::DEFAULT_METRICS_PORT, ipfs_rpc::IpfsRpcClient,
        resource_limits::ResourceLimitsConfig,
    },
    utils::{
        apply_account, apply_cluster_role, apply_cluster_role_binding, apply_config_map,
        apply_service, apply_stateful_set, Clock, Context,
    },
};

pub const OTEL_APP: &str = "otel";
pub const OTEL_SERVICE_NAME: &str = "otel";

pub const OTEL_CR_BINDING: &str = "monitoring-cluster-role-binding";
pub const OTEL_CR: &str = "monitoring-cluster-role";
pub const OTEL_ACCOUNT: &str = "monitoring-service-account";

pub const OTEL_CONFIG_MAP_NAME: &str = "otel-config";

pub struct OtelConfig {
    pub dev_mode: bool,
}

pub async fn apply(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    config: &OtelConfig,
    orefs: &[OwnerReference],
) -> Result<(), kube::error::Error> {
    apply_account(cx.clone(), ns, orefs.to_vec(), OTEL_ACCOUNT).await?;
    apply_cluster_role(cx.clone(), ns, orefs.to_vec(), OTEL_CR, cluster_role()).await?;
    apply_cluster_role_binding(
        cx.clone(),
        orefs.to_vec(),
        OTEL_CR_BINDING,
        cluster_role_binding(ns),
    )
    .await?;
    apply_config_map(
        cx.clone(),
        ns,
        orefs.to_vec(),
        OTEL_CONFIG_MAP_NAME,
        config_map_data(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        orefs.to_vec(),
        OTEL_SERVICE_NAME,
        service_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.to_vec(),
        "opentelemetry",
        stateful_set_spec(config),
    )
    .await?;

    Ok(())
}

fn service_spec() -> ServiceSpec {
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
                name: Some("all-metrics".to_owned()),
                port: DEFAULT_METRICS_PORT,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(DEFAULT_METRICS_PORT)),
                ..Default::default()
            },
            ServicePort {
                name: Some("sim-metrics".to_owned()),
                port: 9465,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(9465)),
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

fn resource_requirements(dev_mode: bool) -> ResourceRequirements {
    if dev_mode {
        ResourceRequirements {
            limits: Some(ResourceLimitsConfig::dev_default().into()),
            requests: Some(ResourceLimitsConfig::dev_default().into()),
            ..Default::default()
        }
    } else {
        ResourceRequirements {
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
        }
    }
}

fn stateful_set_spec(config: &OtelConfig) -> StatefulSetSpec {
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
                    args: Some(vec!["--config=/config/otel-config.yaml".to_owned()]),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: 4317,
                            name: Some("otlp-receiver".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: DEFAULT_METRICS_PORT,
                            name: Some("all-metrics".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 9465,
                            name: Some("sim-metrics".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 8888,
                            name: Some("self-metrics".to_owned()),
                            ..Default::default()
                        },
                    ]),
                    resources: Some(resource_requirements(config.dev_mode)),
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

fn cluster_role() -> ClusterRole {
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

fn cluster_role_binding(ns: &str) -> ClusterRoleBinding {
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

fn config_map_data() -> BTreeMap<String, String> {
    // Include a config that will scrape pods in the network
    BTreeMap::from_iter(vec![(
        "otel-config.yaml".to_owned(),
        r#"---
receivers:
  # Push based metrics
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
  # Pull based metrics
  prometheus:
    config:
      scrape_configs:
        - job_name: 'kubernetes-service-endpoints'
          scrape_interval: 10s
          scrape_timeout: 1s

          kubernetes_sd_configs:
          - role: pod

          # Only container ports named `metrics` will be considered valid targets.
          #
          # Setup relabel rules to give meaning to the following k8s annotations:
          #   prometheus/path - URL path of the metrics endpoint
          #
          # Example:
          #   annotations:
          #      prometheus/path: "/api/v0/metrics"
          relabel_configs:
          - source_labels: [__meta_kubernetes_pod_container_port_name]
            action: keep
            regex: "metrics"
          - source_labels: [__meta_kubernetes_pod_annotation_prometheus_path]
            action: replace
            target_label: __metrics_path__
            regex: (.+)
          - source_labels: [__meta_kubernetes_namespace]
            action: replace
            target_label: kubernetes_namespace
          - source_labels: [__meta_kubernetes_pod_name]
            action: replace
            target_label: kubernetes_pod
          - source_labels: [__meta_kubernetes_pod_container_name]
            action: replace
            target_label: kubernetes_container

processors:
  batch:

exporters:
  # This is unused but can be easily added for debugging.
  logging:
    # can be one of detailed | normal | basic
    verbosity: detailed
    # Log all messages, do not sample
    sampling_initial: 1
    sampling_thereafter: 1
  otlp/jaeger:
    endpoint: jaeger:4317
    tls:
      insecure: true
  prometheus:
    endpoint: 0.0.0.0:9464
    # Keep stale metrics around for 1h before dropping
    # This helps as simulation metrics are stale once the simulation stops.
    metric_expiration: 1h
    resource_to_telemetry_conversion:
      enabled: true
  prometheus/simulation:
    endpoint: 0.0.0.0:9465
    # Keep stale metrics around for 1h before dropping
    # This helps as simulation metrics are stale once the simulation stops.
    metric_expiration: 1h
    resource_to_telemetry_conversion:
      enabled: true

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlp/jaeger]
    metrics:
      receivers: [otlp,prometheus]
      processors: [batch]
      exporters: [prometheus]
    metrics/simulation:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheus/simulation]
  # Enable telemetry on the collector itself
  telemetry:
    logs:
      level: info
    metrics:
      level: detailed
      address: 0.0.0.0:8888"#
            .to_owned(),
    )])
}
