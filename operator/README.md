# Operator Patterns

This document discusses some of the designs patterns of the operator.

## Specs, Statuses, and Configs

The operator is responsible to managing many resources and controlling how those resources can be customized.
As a result the operator adopts a specs, statuses and configs pattern.

* Specs - Defines the desired state.
* Statuses - Reports the current state.
* Configs - Custom configuration to control creating a Spec.

Both specs and statuses are native concepts to Kubernetes.
A spec provides the user facing API for defining their desired state.
A status reports on the actual state.
This code base introduces the concept of a config.

Naturally, operators wrap existing specs and hide some of their details.
However some of those details should be exposed to the user.
A config defines how the parts of a spec owned by the operator can be exposed.
In turn the configs themselves have their own specs, i.e. the API into how to customize internal specs of the operator.

For example the `bootstrap` job requires `JobSpec` to run the job.
The bootstrap job is responsible for telling new peers in the network about existing peers.
Exposing the `JobSpec` to the user puts too much onus on the user to create a functional job.
Instead we define a `BootstrapSpec`, a `BootstrapConfig` and a function that can create the necessary `JobSpec` given a `BootstrapConfig`.
The `BootstrapSpec` is the user API for controlling the bootstrap job.
The `BootstrapConfig` controls which properties of the `JobSpec` can be customized and provides sane defaults.


Let's see how this plays out in the code.
Here is a simplified example of the bootstrap job that allows customizing only the image and bootstrap method:

```rust
// BootstrapSpec defines how the network bootstrap process should proceed.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, JsonSchema)]
pub struct BootstrapSpec {
    // Note, both image and method are optional as the user
    // may want to specify only one or the other or both.
    pub image: Option<String>,
    pub method: Option<String>,
}
// BootstrapConfig defines which properties of the JobSpec can be customized.
pub struct BootstrapConfig {
    // Note, neither image nor method are optional as we need
    // valid values in order to build the JobSpec.
    pub image: String,
    pub method: String,
}
// Define clear defaults for the config.
impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            image: "public.ecr.aws/r5b3e0r5/3box/keramik-runner".to_owned(),
            method: "ring".to_owned(),
        }
    }
}
// Implement a conversion from the spec to the config applying defaults.
impl From<BootstrapSpec> for BootstrapConfig {
    fn from(value: BootstrapSpec) -> Self {
        let default = Self::default();
        Self {
            image: value.image.unwrap_or(default.image),
            method: value.method.unwrap_or(default.method),
        }
    }
}
// Additionally implement the conversion for the case we the entire spec was left undefined.
impl From<Option<BootstrapSpec>> for BootstrapConfig {
    fn from(value: Option<BootstrapSpec>) -> Self {
        match value {
            Some(spec) => spec.into(),
            None => BootstrapConfig::default(),
        }
    }
}
// Define a function that can produce a JobSpec from a config.
pub fn bootstrap_job_spec(config: impl Into<BootstrapConfig>) -> JobSpec {
    let config: BootstrapConfig = config.into();
    // Define the JobSpec using the config, implementation elided.
}
```


Now for the operator reconcile loop we can simply add the `BootstrapSpec` spec to the top level `NetworkSpec` and construct a `JobSpec` to apply.

```rust
pub struct NetworkSpec {
    pub replicas: i32,
    pub bootstrap: Option<BootstrapSpec>,
    // ...
}

pub async fn reconcile(network: Arc<Network>, cx: Arc<ContextData>) -> Result<Action, Error> {
    // ...

    // Now with a single line we go from user defined spec to complete JobSpec
    let spec: JobSpec = bootstrap_job_spec(network.spec().bootstrap);
    apply_job(cx.clone(), ns, network.clone(), BOOTSTRAP_JOB_NAME, spec).await?;

    // ...
}
```


With this pattern it now becomes easy to add more functionallity to the operator by adding a new field to the config and mapping it to the spec.
Additionally by defining the defaults on the config type there is one clear location where defaults are defined and applied, instead of scattering them through the implementation of the spec construction function or elsewhere.


## Assembled Nodes

Another pattern the operator leverages is to assemble the set of nodes instead of relying on determistic behaviors to assume information about nodes.
Assembly is more robust as it is explicit about node information.

In practice this means the operator produces a `keramik-peers` config map for each network.
The config map contains a key `peers.json` which is the JSON serialization of all ready peers with their p2p address and rpc address.
It is expected that other systems consume that config map in order to learn about peers in the network.
The `runner` does exactly this inorder to bootstrap the network.

