use std::sync::Arc;

use futures::stream::StreamExt;
use kube::runtime::watcher::Config;
use kube::ResourceExt;
use kube::{client::Client, runtime::controller::Action, runtime::Controller, Api};
use tokio::time::Duration;
use serde_json::json;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use kube::api::{Patch, PatchParams, PostParams};


#[derive(CustomResource, Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
#[kube(
    group = "simulator.io",
    version = "v1",
    kind = "Network",
    plural = "networks",
    status = "NetworkStatus",
    derive = "PartialEq",
)]
pub struct NetworkSpec {
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
pub struct NetworkStatus {
    enabled: bool,
}

//can add resource apis here 
struct ContextData {
    client: Client,
}

impl ContextData {
    pub fn new(client: Client) -> Self {
        ContextData { client }
    }
}

#[tokio::main]
async fn main() {
    let k_client: Client = Client::try_default().await.unwrap();

    // Add api for other resources, ie ceramic nodes 
    let net_api: Api<Network> = Api::all(k_client.clone());
    let context: Arc<ContextData> = Arc::new(ContextData::new(k_client.clone()));

    Controller::new(net_api.clone(), Config::default())
        .run(reconcile, on_error, context)
        .for_each(|rec_res| async move {
            match rec_res {
                Ok(network_resource) => {
                    println!("Success: {:?}", network_resource);
                }
                Err(rec_err) => {
                    eprintln!("Fail: {:?}", rec_err)
                }
            }
        })
        .await;
}

fn on_error(network: Arc<Network>, error: &Error, _context: Arc<ContextData>) -> Action {
    eprintln!("Rec error:\n{:?}.\n{:?}", error, network);
    Action::requeue(Duration::from_secs(5))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kube error: {source}")]
    KubeError {
        #[from]
        source: kube::Error,
    }
}

async fn reconcile(network: Arc<Network>, context: Arc<ContextData>) -> Result<Action, Error> {
    let client: Client = context.client.clone(); 
    let net_api: Api<Network> = Api::all(client);
    let name = network.name_any(); 

    let status = json!({
        "status": NetworkStatus { enabled: true }
    });

    // TODO fails, assuming status/resource was not created
    net_api.patch_status(&name, &PatchParams::default(), &Patch::Merge(status)).await?;
    // KubeError { source: Api(ErrorResponse { status: "404 Not Found", message: "\"404 page not found\\n\"", reason: "Failed to parse error data", code: 404 }) }.
    // net_api.replace_status(&name, &PostParams::default(),serde_json::to_vec(&status).unwrap()).await?;
    // let status = net_api.get_status(&name).await?;
    // println!("{:?}",status);

    Ok(Action::requeue(Duration::from_secs(30)))
}

// Path status rec error: 
// ReconcilerFailed(
//     KubeError { 
//         source: Api(ErrorResponse { 
//             status: "404 Not Found", 
//             message: "\"404 page not found\\n\"", 
//             reason: "Failed to parse error data", 
//             code: 404 }) 
//     }, 
//     ObjectRef { 
//         dyntype: ApiResource { 
//             group: "simulator.io", 
//             version: "v1", 
//             api_version: "simulator.io/v1",
//             kind: "Network", 
//             plural: "networks" 
//         }, 
//         name: "network", 
//         namespace: Some("default"), 
//         extra: Extra { resource_version: Some("46890"), uid: Some("5945728a-5d77-4555-9636-9e37b1982a4a") } 
//     }
// )