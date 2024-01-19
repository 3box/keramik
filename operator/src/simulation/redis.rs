use std::collections::BTreeMap;

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            Container, ContainerPort, PodSpec, PodTemplateSpec, ResourceRequirements, ServicePort,
            ServiceSpec,
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity, apis::meta::v1::LabelSelector, apis::meta::v1::ObjectMeta,
        util::intstr::IntOrString,
    },
};

use crate::{
    labels::{managed_labels, selector_labels},
    network::resource_limits::ResourceLimitsConfig,
};

pub const REDIS_APP: &str = "redis";

pub fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![ServicePort {
            name: Some("redis-ingress".to_owned()),
            port: 6379,
            protocol: Some("TCP".to_owned()),
            target_port: Some(IntOrString::Int(6379)),
            ..Default::default()
        }]),
        selector: selector_labels(REDIS_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}

pub fn stateful_set_spec(dev_mode: bool) -> StatefulSetSpec {
    StatefulSetSpec {
        replicas: Some(1),
        selector: LabelSelector {
            match_labels: selector_labels(REDIS_APP),
            ..Default::default()
        },
        service_name: REDIS_APP.to_owned(),
        template: PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: selector_labels(REDIS_APP).map(|mut lbls| {
                    lbls.append(&mut managed_labels().unwrap());
                    lbls
                }),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: REDIS_APP.to_owned(),
                    image: Some("redis:latest".to_owned()),
                    image_pull_policy: Some("IfNotPresent".to_string()),
                    ports: Some(vec![ContainerPort {
                        container_port: 6379,
                        name: Some("redis-port".to_owned()),
                        ..Default::default()
                    }]),
                    env: None,
                    resources: Some(resource_requirements(dev_mode)),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        },
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
