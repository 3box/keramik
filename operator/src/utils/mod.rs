//! Utils is shared functions and contants for the controller
use std::{collections::BTreeMap, sync::Arc };

use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetSpec, StatefulSetStatus},
        batch::v1::{Job, JobSpec, JobStatus},
        core::v1::{ ConfigMap, Service, ServiceSpec, ServiceStatus, ServiceAccount },
        rbac::v1::{
			ClusterRole, ClusterRoleBinding
		}
    }, apimachinery::pkg::apis::meta::v1::OwnerReference
};

use kube::{
    api::{ Patch, PatchParams},
    client::Client,
    core::ObjectMeta,
    Api,
};

/// Client Context
pub struct ContextData {
  /// A kube client
  pub client: Client,
}

impl ContextData {
  /// Create a new context
  pub fn new(client: Client) -> Self {
      ContextData { client }
  }
}

/// A list of constants used in various K8s resources
pub const CONTROLLER_NAME: &str = "keramik";

/// Create lables that can be used as a unique selector for a given app name.
pub fn selector_labels(app: &str) -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from_iter(vec![(
        "app".to_owned(),
        app.to_owned(),
    )]))
}

/// Manage by label
pub const MANAGED_BY_LABEL_SELECTOR: &str = "managed-by=keramik";

/// Labels that indicate the resource is managed by the keramik operator.
pub fn managed_labels() -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from_iter(vec![(
        "managed-by".to_owned(),
        "keramik".to_owned(),
    )]))
}

/// Apply a Service
pub async fn apply_service(
  cx: Arc<ContextData>,
  ns: &str,
  orefs: Vec<OwnerReference>,
  name: &str,
  spec: ServiceSpec,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
  let serverside = PatchParams::apply(CONTROLLER_NAME);
  let services: Api<Service> = Api::namespaced(cx.client.clone(), ns);

  // Server-side apply service
  let service: Service = Service {
      metadata: ObjectMeta {
          name: Some(name.to_owned()),
          owner_references: Some(orefs),
          labels: managed_labels(),
          ..ObjectMeta::default()
      },
      spec: Some(spec),
      ..Default::default()
  };
  let service = services
      .patch(name, &serverside, &Patch::Apply(service))
      .await?;
  Ok(service.status)
}

/// Apply a Job 
pub async fn apply_job(
  cx: Arc<ContextData>,
  ns: &str,
  orefs: Vec<OwnerReference>,
  name: &str,
  spec: JobSpec,
) -> Result<Option<JobStatus>, kube::error::Error> {
  let serverside = PatchParams::apply(CONTROLLER_NAME);
  let jobs: Api<Job> = Api::namespaced(cx.client.clone(), ns);

  // Server-side apply stateful_set
  let job: Job = Job {
      metadata: ObjectMeta {
          name: Some(name.to_owned()),
          owner_references: Some(orefs),
          labels: managed_labels(),
          ..ObjectMeta::default()
      },
      spec: Some(spec),
      ..Default::default()
  };
  let job = jobs.patch(name, &serverside, &Patch::Apply(job)).await?;
  Ok(job.status)
}

/// Apply a stateful set in namespace
pub async fn apply_stateful_set(
    cx: Arc<ContextData>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    spec: StatefulSetSpec,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.client.clone(), ns);

    // Server-side apply stateful_set
    let stateful_set: StatefulSet = StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        spec: Some(spec),
        ..Default::default()
    };
    let stateful_set = stateful_sets
        .patch(name, &serverside, &Patch::Apply(stateful_set))
        .await?;
    Ok(stateful_set.status)
}

/// Apply account in namespace
pub async fn apply_account(
    cx: Arc<ContextData>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
) -> Result<ServiceAccount, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let accounts: Api<ServiceAccount> = Api::namespaced(cx.client.clone(), ns);
    // let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.client.clone(), ns);

    // Server-side apply account
    let account: ServiceAccount  = ServiceAccount {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        ..Default::default()
    };
    let account = accounts
        .patch(name, &serverside, &Patch::Apply(account))
        .await?;
    Ok(account)
}

/// Apply cluster role
pub async fn apply_cluster_role(
    cx: Arc<ContextData>,
    _ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    cr: ClusterRole
) -> Result<ClusterRole, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    // let roles: Api<ClusterRole> = Api::namespaced(cx.client.clone(), ns);
    let roles: Api<ClusterRole> = Api::all(cx.client.clone());
    // let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.client.clone(), ns);

    // Server-side apply cluster role
    let role: ClusterRole  = ClusterRole {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..cr.metadata
        },
        ..cr
    };
    let role = roles
        .patch(name, &serverside, &Patch::Apply(role))
        .await?;
    Ok(role)
}

/// Apply cluster role binding
pub async fn apply_cluster_role_binding(
    cx: Arc<ContextData>,
    // TODO
    _ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    crb: ClusterRoleBinding 
) -> Result<ClusterRoleBinding , kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    // let roles: Api<ClusterRole> = Api::namespaced(cx.client.clone(), ns);
    let role_bindings: Api<ClusterRoleBinding > = Api::all(cx.client.clone());
    // let role_bindings: Api<ClusterRoleBinding> = Api::namespaced(cx.client.clone(), ns);

    // Server-side apply cluster role binding
    let role_binding: ClusterRoleBinding = ClusterRoleBinding  {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..crb.metadata
            // ..ObjectMeta::default()
        },
        ..crb
    };
    let role_binding = role_bindings
        .patch(name, &serverside, &Patch::Apply(role_binding))
        .await?;
    Ok(role_binding)
}

/// Apply a config map
pub async fn apply_config_map(
    cx: Arc<ContextData>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    data: BTreeMap<String, String>,
) -> Result<(), kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let config_maps: Api<ConfigMap> = Api::namespaced(cx.client.clone(), ns);
    // Apply config map
    let map_data = ConfigMap {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..ObjectMeta::default()
        },
        data: Some(data),
        ..Default::default()
    };
    config_maps
        .patch(name, &serverside, &Patch::Apply(map_data))
        .await?;
    Ok(())
}