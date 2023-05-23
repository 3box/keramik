//! Utils is shared functions and contants for the controller
use std::{collections::BTreeMap, sync::Arc };

use k8s_openapi::{
    api::{
        batch::v1::{Job, JobSpec, JobStatus},
        core::v1::{ Service, ServiceSpec, ServiceStatus},
    }, apimachinery::pkg::apis::meta::v1::OwnerReference,
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