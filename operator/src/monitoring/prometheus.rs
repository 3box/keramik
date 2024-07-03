use std::{collections::BTreeMap, sync::Arc};

use k8s_openapi::{
    api::{
        apps::v1::StatefulSetSpec,
        core::v1::{
            ConfigMapVolumeSource, Container, ContainerPort, PodSpec, PodTemplateSpec, ResourceRequirements, ServicePort, ServiceSpec, Volume, VolumeMount
        },
    },
    apimachinery::pkg::{
        api::resource::Quantity,
        apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference}, util::intstr::IntOrString,
    },
};
use rand::RngCore;

use crate::{
    network::{ipfs_rpc::IpfsRpcClient, resource_limits::ResourceLimitsConfig},
    utils::{apply_config_map, apply_service, apply_stateful_set, Clock, Context},
};

use crate::labels::selector_labels;

pub const PROM_APP: &str = "prometheus";
pub const PROM_CONFIG_MAP_NAME: &str = "prom-config";
pub const PROM_SERVICE_NAME: &str = "prometheus";

pub struct PrometheusConfig {
    pub dev_mode: bool,
}

pub async fn apply(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    config: &PrometheusConfig,
    orefs: &[OwnerReference],
) -> Result<(), kube::error::Error> {
    apply_config_map(
        cx.clone(),
        ns,
        orefs.to_vec(),
        PROM_CONFIG_MAP_NAME,
        config_map_data(),
    )
    .await?;
    apply_service(
    cx.clone(),
    ns,
    orefs.to_vec(),
    PROM_SERVICE_NAME,
    service_spec(),
)
.await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.to_vec(),
        "prometheus",
        stateful_set_spec(config.dev_mode),
    )
    .await?;
    Ok(())
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

fn service_spec() -> ServiceSpec {
    ServiceSpec {
        ports: Some(vec![
            ServicePort {
                name: Some("prometheus".to_owned()),
                port: 9090,
                protocol: Some("TCP".to_owned()),
                target_port: Some(IntOrString::Int(9090)),
                ..Default::default()
            },
        ]),
        selector: selector_labels(PROM_APP),
        type_: Some("ClusterIP".to_owned()),
        ..Default::default()
    }
}

fn stateful_set_spec(dev_mode: bool) -> StatefulSetSpec {
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
                    image: Some("prom/prometheus:v2.45.6".to_owned()),
                    command: Some(vec![
                        "/bin/prometheus".to_owned(),
                        "--web.enable-lifecycle".to_owned(),
                        "--web.enable-remote-write-receiver".to_owned(),
                        "--config.file=/config/prom-config.yaml".to_owned(),
                    ]),
                    ports: Some(vec![ContainerPort {
                        container_port: 9090,
                        name: Some("webui".to_owned()),
                        ..Default::default()
                    }]),
                    resources: Some(resource_requirements(dev_mode)),
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

fn config_map_data() -> BTreeMap<String, String> {
    let config_str = include_str!("./prom-config.yaml");

    BTreeMap::from_iter(vec![(
        "prom-config.yaml".to_owned(),
        config_str.to_owned(),
    )])
}
