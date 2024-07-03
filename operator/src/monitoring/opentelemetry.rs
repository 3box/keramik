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
        rbac::v1::{PolicyRule, RoleRef, Subject, Role, RoleBinding},
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
        apply_account, apply_namespaced_role, apply_namespaced_role_binding, apply_config_map,
        apply_service, apply_stateful_set, Clock, Context,
    },
};

pub const OTEL_APP: &str = "otel";
pub const OTEL_SERVICE_NAME: &str = "otel";

pub const OTEL_ROLE_BINDING: &str = "monitoring-role-binding";
pub const OTEL_ROLE: &str = "monitoring-role";
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
    apply_namespaced_role(cx.clone(), ns, orefs.to_vec(), OTEL_ROLE, namespace_role()).await?;
    apply_namespaced_role_binding(
        cx.clone(),
        ns,
        orefs.to_vec(),
        OTEL_ROLE_BINDING,
        role_binding(ns),
    ).await?;
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
                    image: Some("otel/opentelemetry-collector-contrib:0.104.0".to_owned()),
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


fn config_map_data() -> BTreeMap<String, String> {
    let config_str = include_str!("./otel-config.yaml"); // Adjust the path as necessary
    BTreeMap::from_iter(vec![("otel-config.yaml".to_owned(), config_str.to_owned())])
}

fn namespace_role() -> Role {
    Role {
        rules: Some(vec![PolicyRule {
            api_groups: Some(vec!["".to_owned()]),
            resources: Some(vec!["pods".to_owned()]),
            verbs: vec!["get".to_owned(), "list".to_owned(), "watch".to_owned()],
            ..Default::default()
        }]),
        ..Default::default()
    }
}

fn role_binding(ns: &str) -> RoleBinding {
    RoleBinding {
        role_ref: RoleRef {
            kind: "Role".to_owned(),
            name: OTEL_ROLE.to_owned(),
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
