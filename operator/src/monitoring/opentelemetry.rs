use std::collections::BTreeMap;

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
        api::resource::Quantity, apis::meta::v1::LabelSelector, apis::meta::v1::ObjectMeta,
        util::intstr::IntOrString,
    },
};

use crate::labels::selector_labels;

use crate::simulation::controller::{OTEL_ACCOUNT, OTEL_CONFIG_MAP_NAME, OTEL_CR};

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

pub fn stateful_set_spec() -> StatefulSetSpec {
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

pub fn config_map_data() -> BTreeMap<String, String> {
    BTreeMap::from_iter(vec![(
        "otel-config.yaml".to_owned(),
        r#"
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
        endpoint: 0.0.0.0:9090
        # Keep stale metrics around for 1h before dropping
        # This helps as simulation metrics are stale once the simulation stops.
        metric_expiration: 1h
        resource_to_telemetry_conversion: 
          enabled: true
      parquet:
        path: /data/
      # Grafana Cloud export
      # TODO: Remove, this work however its not possible to
      # namespace the metrics from other Grafana metrics which makes
      # it hard to consume and polutes the normal metrics namespace.
      #
      # For now leaving this here as an example of how to enable,
      # but will rely on local prometheus metrics in the short term.
      #otlphttp/grafana:
      #  auth:
      #    authenticator: basicauth/grafana
      #  endpoint: https://otlp-gateway-prod-us-central-0.grafana.net/otlp
    
            #extensions:
            #  basicauth/grafana:
            #    client_auth:
            #      username: "user" # replace with Grafana instance id
            #      password: "password" # replace with Grafana API token (via a secret)
    
    service:
      #extensions: [basicauth/grafana]
      pipelines:
        traces:
          receivers: [otlp]
          processors: [batch]
          exporters: [otlp/jaeger]
        metrics:
          receivers: [otlp,prometheus]
          processors: [batch]
          exporters: [parquet, prometheus]
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
