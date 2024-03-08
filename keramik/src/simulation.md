# Simulation

To run a simulation, first define a simulation. Available simulation types are

- `ipfs-rpc` - A simple simulation that writes and reads to IPFS
- `ceramic-simple` - A simple simulation that writes and reads events to two different streams, a small and large model
- `ceramic-write-only` - A simulation that only performs updates on two different streams
- `ceramic-new-streams` - A simulation that only creates new streams
- `ceramic-model-reuse` - A simulation that reuses the same model and queries instances across workers
- `recon-event-key-sync` - A simulation that creates event keys for Recon to sync at a fixed rate (~300/s by default). Designed for a 2 node network but should work on any.
- `recon-event-sync` - A simulation that creates events for Recon to sync at a fixed rate (~300/s by default). Designed for a 2 node network but should work on any. Choosing between recon scenarios depends on what version of rust-ceramic you have. Some versions support keys only, some support key/value (maybe we'll support both someday). If you're not sure, try this one first.

Using one of these scenarios, we can then define the configuration for that scenario:

```yaml
# basic.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Simulation
metadata:
  name: basic
  # Must be the same namespace as the network to test
  namespace: keramik-<unique-name>-small
spec:
  scenario: ceramic-simple
  network: local
  devMode: true # optional to remove container resource limits and requirements for local benchmarking
  users: 10
  runTime: 4
```

If you want to run it against a defined network, set the namespace to the same as the network. in this example the
namespace is set to the same network applied when [the network was setup](./setup_network.md).
Additionally, you can define the scenario you want to run, the number of users to run for each node, and the number of minutes it will run.

Before running the simulation make sure the `network` is ready and has [monitoring](./monitoring.md) enabled.

```
kubectl describe network <unique-name>-small
```

You should see that the number of `Ready Replicas` is the same as the `Replicas`.
Example simplified output of a ready network:

```txt
Name:         nc-small
...
  Ready Replicas:  2
  Replicas:        2
...
```


Once ready, apply this simulation defintion to the k8s cluster:

```shell
kubectl apply -f basic.yaml
```

Keramik will first start all the metrics and tracing resources, once ready it will start the simulation by first starting the simulation manager and then all the workers.
The manager and workers will stop once the simulation is complete.

You can then [analyze](analysis.md) the results of the simulation.

If you want to rerun a simulation with no changes, you can delete the simulation and reapply it.

```shell
kubectl delete -f basic.yaml
```

## Simulating Specific Versions

Often you will want to run a simulation against a specific version of software.
To do this you will need to build the image and configure your network to run that image.

### Example Custom JS-Ceramic Image

Use this example `network` definition with a custom `js-ceramic` image.

```yaml
# custom-js-ceramic.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: custom-js-ceramic
spec:
  replicas: 2
  monitoring:
    namespaced: true
  ceramic:
    - image: ceramicnetwork/composedb:dev
      imagePullPolicy: IfNotPresent
```

```shell
kubectl apply -f custom-js-ceramic.yaml
```

You can also run [mixed networks](./mixed_networks.md) and various other [advanced](./advanced_configuration.md) configurations.


### Example Custom IPFS Image

Use this example `network` definition with a custom `IPFS` image.

```yaml
# custom-ipfs.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: custom-ipfs
spec:
  replicas: 2
  monitoring:
    namespaced: true
  ceramic:
    - ipfs:
        rust:
          image: ceramicnetwork/rust-ceramic:dev
          imagePullPolicy: IfNotPresent
```

```shell
kubectl apply -f custom-ipfs.yaml
```

