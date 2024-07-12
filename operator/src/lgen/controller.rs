use std::{sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::api::{batch::v1::Job};
use kube::{
    api::{Patch, PatchParams},
    client::Client,
    core::object::HasSpec,
    runtime::Controller,
    Api,
};
use kube::{
    runtime::{
        controller::Action,
        watcher::{self, Config},
    },
    Resource, ResourceExt,
};
use opentelemetry::{global, KeyValue};
use rand::{distributions::Alphanumeric, thread_rng, Rng, RngCore};

use tracing::{debug, error, info};

use crate::{
    labels::MANAGED_BY_LABEL_SELECTOR, lgen::{
        job::{JobConfig, JobImageConfig, job_spec}, spec::{LoadGenerator, LoadGeneratorStatus},
    }, simulation::controller::{get_num_peers, monitoring_ready}, utils::Clock
};

use crate::network::{
    ipfs_rpc::{HttpRpcClient, IpfsRpcClient},
};


use crate::utils::{apply_job, apply_service, apply_stateful_set, Context};

pub const LOAD_GENERATOR_JOB_NAME: &str = "load-gen-job";

/// Handle errors during reconciliation.
fn on_error(
    _network: Arc<LoadGenerator>,
    _error: &Error,
    _context: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
) -> Action {
    Action::requeue(Duration::from_secs(5))
}

/// Errors produced by the reconcile function.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("App error: {source}")]
    App {
        #[from]
        source: anyhow::Error,
    },
    #[error("Kube error: {source}")]
    Kube {
        #[from]
        source: kube::Error,
    },
}

/// Start a controller for the LoadGenerator CRD.
pub async fn run() {
    let k_client = Client::try_default().await.unwrap();
    let context = Arc::new(
        Context::new(k_client.clone(), HttpRpcClient).expect("should be able to create context"),
    );

    let load_generators: Api<LoadGenerator> = Api::all(k_client.clone());
    let jobs = Api::<Job>::all(k_client.clone());

    Controller::new(load_generators.clone(), Config::default())
        .owns(
            jobs,
            watcher::Config::default().labels(MANAGED_BY_LABEL_SELECTOR),
        )
        .run(reconcile, on_error, context)
        .for_each(|rec_res| async move {
            match rec_res {
                Ok((load_generator, _)) => {
                    info!(load_generator.name, "reconcile success");
                }
                Err(err) => {
                    error!(?err, "reconcile error")
                }
            }
        })
        .await;
}

/// Perform a reconcile pass for the LoadGenerator CRD
async fn reconcile(
    load_generator: Arc<LoadGenerator>,
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
) -> Result<Action, Error> {
    let meter = global::meter("keramik");
    let runs = meter
        .u64_counter("load_generator_reconcile_count")
        .with_description("Number of load generator reconciles")
        .init();

    match reconcile_(load_generator, cx).await {
        Ok(action) => {
            runs.add(
                1,
                &[KeyValue {
                    key: "result".into(),
                    value: "ok".into(),
                }],
            );
            Ok(action)
        }
        Err(err) => {
            runs.add(
                1,
                &[KeyValue {
                    key: "result".into(),
                    value: "err".into(),
                }],
            );
            Err(err)
        }
    }
}

/// Perform a reconcile pass for the LoadGenerator CRD
async fn reconcile_(
    load_generator: Arc<LoadGenerator>,
    cx: Arc<Context<impl IpfsRpcClient, impl RngCore, impl Clock>>,
) -> Result<Action, Error> {
    let spec = load_generator.spec();

    let status = if let Some(status) = &load_generator.status {
        status.clone()
    } else {
        // Generate new status with random name and nonce
        LoadGeneratorStatus {
            nonce: thread_rng().gen(),
            name: "load-gen-"
                .chars()
                .chain(
                    thread_rng()
                        .sample_iter(&Alphanumeric)
                        .take(6)
                        .map(char::from),
                )
                .collect::<String>(),
        }
    };
    debug!(?spec, ?status, "reconcile");

    let ns = load_generator.namespace().unwrap();
    let num_peers = get_num_peers(cx.clone(), &ns).await?;

    // The load generator does not deploy the monitoring resources but they must exist in order to
    // collect the results of load generators.
    let ready = monitoring_ready(cx.clone(), &ns).await?;

    if !ready {
        return Ok(Action::requeue(Duration::from_secs(10)));
    }


    let job_image_config = JobImageConfig::from(spec);

    let job_config = JobConfig {
        name: status.name.clone(),
        scenario: spec.scenario.to_owned(),
        users: spec.users.to_owned(),
        run_time: spec.run_time.to_owned(),
        nonce: status.nonce,
        job_image_config: job_image_config.clone(),
        throttle_requests: spec.throttle_requests,
    };
    let orefs = load_generator
        .controller_owner_ref(&())
        .map(|oref| vec![oref])
        .unwrap_or_default();

    apply_job(cx.clone(), &ns, orefs.clone(), LOAD_GENERATOR_JOB_NAME, job_spec(job_config)).await?;

    let load_generators: Api<LoadGenerator> = Api::namespaced(cx.k_client.clone(), &ns);
    let _patched = load_generators
        .patch_status(
            &load_generator.name_any(),
            &PatchParams::default(),
            &Patch::Merge(serde_json::json!({ "status": status })),
        )
        .await?;

    Ok(Action::requeue(Duration::from_secs(10)))
}