# Keramik

Keramik is a Kubernetes operator for simulating Ceramic networks.

The `k8s` directory contains the kubernetes manifests for deploying Keramik.


## Setup Kubernetes

Keramik can be used locally or via a cloud Kubernetes service.

### Local deployment

Requires

- [rust](https://rustup.rs/)
- [kind](https://kind.sigs.k8s.io/)
- [kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/)
- [docker](https://docs.docker.com/get-docker/)
- [protoc](https://grpc.io/docs/protoc-installation/)


Kind (Kubernetes in Docker) runs a local k8s cluster. Create and initialize a new kind cluster using this configuration:

```yaml
# kind.yaml
---
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
featureGates:
  MaxUnavailableStatefulSet: true
```

This configuration enables a feature that allows stateful sets to more rapidly redeploy pods on changes.
While not required to use keramik it makes deploying and mutating networks significantly faster.

```
# Create a new kind cluster (i.e. local k8s)
kind create cluster --config kind.yaml
```

### AWS EKS

Login to the EKS cluster using this [guide](https://docs.aws.amazon.com/eks/latest/userguide/create-kubeconfig.html)

## Deploy a Ceramic network

First we need to deploy keramik in order to create and manage a Ceramic network:

    kubectl create namespace keramik
    cargo run --bin crdgen | kubectl create -f - # Create CRDs
    kubectl apply -k ./k8s/operator/             # Start up keramik operator

With the operator running we can now define a Ceramic network.
Place the following network definition into the file `small.yaml`.

```yaml
# small.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
```

Apply this network definition to the k8s cluster:

    kubectl apply -f small.yaml

After a minute or two you should have a functioning Ceramic network.
Check the status of the network:

    kubectl describe network small

Keramik places each network into its own namespace named after the name of the network.
Inspect the pods within the network using:

    kubectl -n keramik-small get pods

>HINT: Use tools like [kubectx](https://github.com/ahmetb/kubectx) or [kubie](https://github.com/sbstp/kubie) to work with multiple namespaces and contexts.

## Simulation

To run a simulation, first define a simulation.
```yaml
# basic.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Simulation
metadata:
  name: basic
  namespace: keramik-small
spec:
  scenario: ceramic-simple
  users: 10
  run_time: 4
```
If you want to run it against a defined network, set the namespace to the same as the network. in this example the namespace is set to the same network applied above "keramik-small".
Additionally, you can define the scenario you want to run, the number of users, and the number of minutes it will run. 

Once ready, apply this simulation defintion to the k8s cluster: 

    kubectl apply -f basic.yaml

Keramik will first start all the metrics and tracing resources, once ready it will start the simulation by first starting the simulation manager and then all the workers. 
The manager and workers will stop once the simulation is complete. 

## Contributing

Contributions are welcome! Opening an issue to disucss your idea is a good first step.
When you are ready please use [convential commit](https://www.conventionalcommits.org/en/v1.0.0/)  messages in your commits and PR titles.

Keramik is composed of two main components:

* Runner - short lived process that performs various tasks within the network (i.e. bootstrapping)
* Operator - long lived process that manages the network custom resource.


### Runner

The `runner` is a utility for running various jobs to initialize the network and run workloads against it.
Any changes to the runner require that you rebuild it and load it into kind again.

    docker buildx build --load -t keramik/runner:dev --target runner .
    kind load docker-image keramik/runner:dev

Now we need to tell the operator to use this new version of the runner.
Edit `small.yaml` to configure the image of the bootstrap runner.

```yaml
# small.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  # Use custom runner image for bootstrapping
  bootstrap:
    image: keramik/runner:dev
    imagePullPolicy: IfNotPresent
```


### Operator

The `operator` automates creating and manipulating networks via custom resource definition.
Any changes to the operator require that you rebuild it and load it into kind again.

    docker buildx build --load -t keramik/operator:dev --target operator .
    kind load docker-image keramik/operator:dev

Next edit `./k8s/operator/kustomization.yaml` to use the `dev` tag

```yaml
images:
  - name: keramik/operator
    newTag: dev
```

Finally apply these changes:

    $ kubectl apply -k ./k8s/operator/

See the [operator/README.md](https://github.com/3box/keramik/blob/main/operator/README.md) for details on certain design patterns of the operator.

## IPFS

The IPFS behavior used by Ceramic can be customized.

### Rust IPFS

Example network config that uses Rust based IPFS (i.e. ceramic-one) with its defaults.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-kubo
spec:
  replicas: 5
  ceramic:
    ipfs:
      kind: rust
```

Example network config that uses Rust based IPFS (i.e. ceramic-one) with a specific image.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-kubo
spec:
  replicas: 5
  ceramic:
    ipfs:
      kind: rust
      image: rust-ceramic/ceramic-one:dev
      imagePullPolicy: IfNotPresent
```

### Kubo IPFS

Example network config that uses Go based IPFS (i.e. Kubo) with its defaults.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-kubo
spec:
  replicas: 5
  ceramic:
    ipfs:
      kind: go
```

Example network config that uses Go based IPFS (i.e. Kubo) with a specific image.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-kubo
spec:
  replicas: 5
  ceramic:
    ipfs:
      kind: go
      image: ceramic/go-ipfs:dev-validator
      imagePullPolicy: IfNotPresent
```

## Opentelemetry

To view the metrics and traces port-forward the services:

    kubectl port-forward prometheus-0 9090
    kubectl port-forward jaeger-0 16686

Then navigate to http://localhost:9090 for metrics and http://localhost:16686 for traces.

## Analysis

To analyze the results of a simulation first copy the metrics-TIMESTAMP.parquet file from the otel-0 pod.
First restart otel-0 pod so it writes out the parquet file footer.

    kubectl delete pod otel-0
    kubectl exec otel-0 -- ls -la /data # List files in the directly find the TIMESTAMP you need
    kubectl cp otel-0:data/metrics-TIMESTAMP.parquet ./analyze/metrics.parquet
    cd analyze

Use duckdb to examine the data:

    duckdb
    > SELECT * FROM 'metrics.parquet' LIMIT 10;

Alternatively start a jupyter notebook using `analyze/sim.ipynb`:

    jupyter notebook


## Comparing Simulation Runs

How do we conclude a simulation is better or worse that another run?

Each simulation will likely be targeting a specific result however there are common results we should expect to see.

Changes should not make correctness worse. Correctness is defined using two metrics:

- Percentage of events successfully persisted on the node that accepted the initial write.
- Percentage of events successfully replicated on nodes that observed the writes via the Ceramic protocol.


Changes should not make performance worse. Performance is defined using these metrics:

- Writes/sec across all nodes in the cluster and by node
- p50,p90,p95,p99 and p99.9 of the duration of writes across all nodes in the cluster and by node
- Success/failure ratio of writes requests across all nodes in the cluster and by node
- p50,p90,p95,p99 and p99.9 of duration of time to become replicated. The time from when one node accepts the write to when another node has the same write available for read.


For any simulation of the Ceramic protocol these metrics should apply. Any report about the results of a simulation should include these metrics and we compare them against the established a baseline.
