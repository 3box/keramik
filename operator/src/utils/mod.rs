//! Utils is shared functions and constants for the controller
use std::sync::Mutex;

#[cfg(test)]
pub mod test;

use std::{collections::BTreeMap, sync::Arc};

use k8s_openapi::{
    api::{
        apps::v1::{StatefulSet, StatefulSetSpec, StatefulSetStatus},
        batch::v1::{Job, JobSpec, JobStatus},
        core::v1::{ConfigMap, EnvVar, Service, ServiceAccount, ServiceSpec, ServiceStatus},
        rbac::v1::{ClusterRole, ClusterRoleBinding},
    },
    apimachinery::pkg::apis::meta::v1::OwnerReference,
    chrono::{DateTime, Utc},
};

use crate::labels::managed_labels_extend;
use crate::{labels::managed_labels, network::ipfs_rpc::IpfsRpcClient, CONTROLLER_NAME};

use kube::{
    api::{DeleteParams, Patch, PatchParams},
    client::Client,
    core::ObjectMeta,
    Api,
};

use rand::{rngs::StdRng, thread_rng, RngCore, SeedableRng};

use anyhow::Result;

/// Operator Context
pub struct Context<R, Rng, C> {
    /// Kube client
    pub k_client: Client,
    /// IPFS client
    pub rpc_client: R,
    /// Random number generator
    pub rng: Mutex<Rng>,
    /// Clock that provide the current time
    pub clock: C,
}

impl<R> Context<R, StdRng, UtcClock> {
    /// Create new context
    pub fn new(k_client: Client, rpc_client: R) -> Result<Self>
    where
        R: IpfsRpcClient,
    {
        Ok(Context {
            k_client,
            rpc_client,
            rng: Mutex::new(StdRng::from_rng(thread_rng())?),
            clock: UtcClock,
        })
    }
}

/// Provides the current time.
pub trait Clock {
    /// Report the current time.
    fn now(&self) -> DateTime<Utc>;
}

/// Provides the current time using real time.
pub struct UtcClock;
impl Clock for UtcClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
/// Apply a Service with extra labels
pub async fn apply_service_with_labels(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    spec: ServiceSpec,
    labels: Option<BTreeMap<String, String>>,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let services: Api<Service> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply service
    let service: Service = Service {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels_extend(labels),
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
/// Apply a Service with default managed labels
pub async fn apply_service(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    spec: ServiceSpec,
) -> Result<Option<ServiceStatus>, kube::error::Error> {
    apply_service_with_labels(cx, ns, orefs, name, spec, None).await
}
/// Delete a service in namespace
pub async fn delete_service(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    name: &str,
) -> Result<(), kube::error::Error> {
    let services: Api<Service> = Api::namespaced(cx.k_client.clone(), ns);

    match services.delete(name, &DeleteParams::default()).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(err)) if err.reason == "NotFound" => Ok(()),
        Err(e) => Err(e),
    }
}

/// Apply a Job
pub async fn apply_job(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    spec: JobSpec,
) -> Result<Option<JobStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let jobs: Api<Job> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply job
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

/// Apply a stateful set in namespace with extra labels
pub async fn apply_stateful_set_with_labels(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    spec: StatefulSetSpec,
    labels: Option<BTreeMap<String, String>>,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply stateful_set
    let stateful_set: StatefulSet = StatefulSet {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels_extend(labels),
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
/// Apply a stateful set in namespace with default managed labels
pub async fn apply_stateful_set(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    spec: StatefulSetSpec,
) -> Result<Option<StatefulSetStatus>, kube::error::Error> {
    apply_stateful_set_with_labels(cx, ns, orefs, name, spec, None).await
}

/// Delete a stateful set in namespace
pub async fn delete_stateful_set(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    name: &str,
) -> Result<(), kube::error::Error> {
    let stateful_sets: Api<StatefulSet> = Api::namespaced(cx.k_client.clone(), ns);

    match stateful_sets.delete(name, &DeleteParams::default()).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(err)) if err.reason == "NotFound" => Ok(()),
        Err(e) => Err(e),
    }
}

/// Apply account in namespace
pub async fn apply_account(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
) -> Result<ServiceAccount, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let accounts: Api<ServiceAccount> = Api::namespaced(cx.k_client.clone(), ns);

    // Server-side apply account
    let account: ServiceAccount = ServiceAccount {
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
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    _ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    cr: ClusterRole,
) -> Result<ClusterRole, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let roles: Api<ClusterRole> = Api::all(cx.k_client.clone());

    // Server-side apply cluster role
    let role: ClusterRole = ClusterRole {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..cr.metadata
        },
        ..cr
    };
    let role = roles.patch(name, &serverside, &Patch::Apply(role)).await?;
    Ok(role)
}

/// Apply cluster role binding
pub async fn apply_cluster_role_binding(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    orefs: Vec<OwnerReference>,
    name: &str,
    crb: ClusterRoleBinding,
) -> Result<ClusterRoleBinding, kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let role_bindings: Api<ClusterRoleBinding> = Api::all(cx.k_client.clone());

    // Server-side apply cluster role binding
    let role_binding: ClusterRoleBinding = ClusterRoleBinding {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            owner_references: Some(orefs),
            labels: managed_labels(),
            ..crb.metadata // ..ObjectMeta::default()
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
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    ns: &str,
    orefs: Vec<OwnerReference>,
    name: &str,
    data: BTreeMap<String, String>,
) -> Result<(), kube::error::Error> {
    let serverside = PatchParams::apply(CONTROLLER_NAME);
    let config_maps: Api<ConfigMap> = Api::namespaced(cx.k_client.clone(), ns);
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

/// Generate a random, hex-encoded secret
pub fn generate_random_secret(
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
    len: usize,
) -> String {
    let mut secret_bytes: Vec<u8> = vec![0; len];
    let mut rng = cx.rng.lock().expect("should be able to acquire lock");
    rng.fill_bytes(&mut secret_bytes);
    hex::encode(secret_bytes)
}

/// Apply override env vars to an existing env var list
pub fn override_env_vars(env: &mut Vec<EnvVar>, overrides: &Option<BTreeMap<String, String>>) {
    if let Some(override_env) = &overrides {
        override_env.iter().for_each(|(key, value)| {
            if let Some((pos, _)) = env.iter().enumerate().find(|(_, var)| &var.name == key) {
                env.swap_remove(pos);
            }
            env.push(EnvVar {
                name: key.to_string(),
                value: Some(value.to_string()),
                ..Default::default()
            })
        });
        // Sort env vars so we can have stable tests
        env.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    }
}
