//! OTEL Resources

use std::sync::Arc;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use rand::RngCore;

use crate::{
    network::utils::IpfsRpcClient,
    utils::{
        apply_account, apply_cluster_role, apply_cluster_role_binding, apply_config_map,
        apply_service, apply_stateful_set, Context,
    },
};

pub(crate) mod jaeger;
pub(crate) mod opentelemetry;
pub(crate) mod prometheus;

const JAEGER_SERVICE_NAME: &str = "jaeger";
const OTEL_SERVICE_NAME: &str = "otel";

const OTEL_CR_BINDING: &str = "monitoring-cluster-role-binding";
const OTEL_CR: &str = "monitoring-cluster-role";
const OTEL_ACCOUNT: &str = "monitoring-service-account";

const OTEL_CONFIG_MAP_NAME: &str = "otel-config";
const PROM_CONFIG_MAP_NAME: &str = "prom-config";

/// Apply jaeger resources into a namespace.
pub async fn apply_jaeger(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
) -> Result<(), kube::error::Error> {
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        JAEGER_SERVICE_NAME,
        jaeger::service_spec(),
    )
    .await?;

    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "jaeger",
        jaeger::stateful_set_spec(),
    )
    .await?;
    Ok(())
}

/// Apply prometheus resources into a namespace.
pub async fn apply_prometheus(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
) -> Result<(), kube::error::Error> {
    apply_config_map(
        cx.clone(),
        ns,
        orefs.clone(),
        PROM_CONFIG_MAP_NAME,
        prometheus::config_map_data(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "prometheus",
        prometheus::stateful_set_spec(),
    )
    .await?;
    Ok(())
}

/// Apply opentelemetry resources into a namespace.
pub async fn apply_opentelemetry(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
) -> Result<(), kube::error::Error> {
    apply_account(cx.clone(), ns, orefs.clone(), OTEL_ACCOUNT).await?;
    apply_cluster_role(
        cx.clone(),
        ns,
        orefs.clone(),
        OTEL_CR,
        opentelemetry::cluster_role(),
    )
    .await?;
    apply_cluster_role_binding(
        cx.clone(),
        orefs.clone(),
        OTEL_CR_BINDING,
        opentelemetry::cluster_role_binding(ns),
    )
    .await?;
    apply_config_map(
        cx.clone(),
        ns,
        orefs.clone(),
        OTEL_CONFIG_MAP_NAME,
        opentelemetry::config_map_data(),
    )
    .await?;
    apply_service(
        cx.clone(),
        ns,
        orefs.clone(),
        OTEL_SERVICE_NAME,
        opentelemetry::service_spec(),
    )
    .await?;
    apply_stateful_set(
        cx.clone(),
        ns,
        orefs.clone(),
        "opentelemetry",
        opentelemetry::stateful_set_spec(),
    )
    .await?;

    Ok(())
}
