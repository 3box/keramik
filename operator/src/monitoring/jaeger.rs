use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            Container, ContainerPort, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements,
            ServicePort, ServiceSpec,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, apis::meta::v1::ObjectMeta,
        util::intstr::IntOrString,
    },
};

use crate::labels::selector_labels;

pub const JAEGER_APP: &str = "jaeger";

pub fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("otlp-receiver".to_owned()),
            port: 4317,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(4317)),
            ..Default::default()
        }]),
        selector: selector_labels(JAEGER_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}

pub fn stateful_set_spec() -> StatefulSetSpec {
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(JAEGER_APP),
            ..Default::default()
        },
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(JAEGER_APP),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "jaeger".to_owned(),
                    image: Some("jaegertracing/all-in-one:latest".to_owned()),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: 4317,
                            name: Some("otlp-receiver".to_owned()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 16686,
                            name: Some("webui".to_owned()),
                            ..Default::default()
                        },
                    ]),
                    env: Some(vec![EnvVar {
                        name: "COLLECTOR_OTLP_ENABLED".to_owned(),
                        value: Some("true".to_owned()),
                        ..Default::default()
                    }]),
                    resources: Some(ResourceRequirements {
                        limits: Some(BTreeMap::from_iter(vec![(
                            "ephemeral-storage".to_owned(),
                            Quantity("1Gi".to_owned()),
                        )])),
                        requests: Some(BTreeMap::from_iter(vec![(
                            "ephemeral-storage".to_owned(),
                            Quantity("1Gi".to_owned()),
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        },
        ..Default::default()
    }
}
