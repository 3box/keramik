use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            ConfigMapVolumeSource, Container, ContainerPort, PodSpec, PodTemplateSpec,
            ResourceRequirements, Volume, VolumeMount,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, apis::meta::v1::ObjectMeta,
    },
};

use crate::monitoring::PROM_CONFIG_MAP_NAME;

use crate::utils::selector_labels;

pub const PROM_APP: &str = "prometheus";

pub fn stateful_set_spec() -> StatefulSetSpec {
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(PROM_APP),
            ..Default::default()
        },
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(PROM_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "prometheus".to_owned(),
                    image: Some("prom/prometheus:v2.42.0".to_owned()),
                    command: Some(vec![
                        "/bin/prometheus".to_owned(),
                        "--web.enable-lifecycle".to_owned(),
                        "--config.file=/config/prom-config.yaml".to_owned(),
                    ]),
                    ports: Some(vec![ContainerPort {
                        container_port: 9090,
                        name: Some("webui".to_owned()),
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
                        mount_path: "/config".to_owned(),
                        name: "config".to_owned(),
                        read_only: Some(true),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                volumes: Some(vec![Volume {
                    config_map: Some(ConfigMapVolumeSource {
                        // TODO ?, how to create config map?
                        default_mode: Some(0o755),
                        name: Some(PROM_CONFIG_MAP_NAME.to_owned()),
                        ..Default::default()
                    }),
                    name: "config".to_owned(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        },
        ..Default::default()
    }
}

pub fn config_map_data() -> BTreeMap<String, String> {
    BTreeMap::from_iter(vec![(
        "prom-config.yaml".to_owned(),
        r#"
        global:
          scrape_interval: 10s
          scrape_timeout: 5s
        
        scrape_configs:
          - job_name: services
            metrics_path: /metrics
            honor_labels: true
            static_configs:
              - targets:
                - 'localhost:9090'
                - 'otel:9090'
                - 'otel:8888'"#
            .to_owned(),
    )])
}
