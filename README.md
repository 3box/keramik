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

```
kind create cluster # Create a new kind cluster (i.e. local k8s)
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
apiVersion: "keramik.3box.io/v1"
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
Edit `small.yaml` to configure the image of the runner.

```yaml
# small.yaml
---
apiVersion: "keramik.3box.io/v1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  # Change the runner image to our locally built one
  runner_image: keramik/runner:dev
  # Change the pull policy to not pull since `kind` load
  # already made the image available and the image only exists locally.
  runner_image_pull_policy: IfNotPresent
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

See the [operator/README.md](https://github.com/3box/keramik/blob/main/operator/README.md) for details on certain design patterns of the operator.

## Opentelemetry

Add opentelemetry collector to a specific newtork.
First edit `./k8s/opentelemetry/kustomization.yaml` and change the namespace to be the namespace of your network (i.e. `keramik-small`).
Then run the following command to add opentelemetry to that network.

    kubectl apply -k ./k8s/opentelemetry/

To view the metrics and traces port-forward the services:

    kubectl port-forward prometheus-0 9090
    kubectl port-forward jaeger-0 16686

Then navigate to http://localhost:9090 for metrics and http://localhost:16686 for traces.

## Simulation

To run a simulation delete the old jobs and re-apply the simulation job definitions:

    kubectl delete jobs.batch simulate-manager simulate-worker-{0..9}
    kubectl apply -k k8s/ceramic/

> NOTE: We will need the k8s operator to make job configuration dynamic.
> This is a manual method in the meantime.

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




