# Keramik

Keramik is a Kubernetes operator for simulating Ceramic networks.

The `k8s` directory contains the kubernetes manifests for a Ceramic network.

The manifests require secrets for a Postgres database and a Ceramic node private key.
An example of creating random secrets is in `create-secrets.sh`.

Overlays:
- overlays/ceramic-hds - an environment with an extra runner container and schemas to test historical data sync.

## Local deployemnt

Requires
  - [kind](https://kind.sigs.k8s.io/)
  - [kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl/)
  - [docker](https://docs.docker.com/get-docker/)


```
# Create a new kind cluster (i.e. local k8s)
kind create cluster
kubectl create ns keramik-0
# Build the runner image and load it into kind
docker build -t keramik/runner:dev runner/
kind load docker-image keramik/runner:dev
# Create new random secrets
./k8s/ceramic/create-secrets.sh
# Start up the network
kubectl apply -k ./k8s/ceramic
```

View logs

```
kubectl logs ceramic-0 -c ceramic
```

## AWS EKS

Keramik can also be deployed against an AWS EKS cluster.
This process is much the same, however the container images must be accessible to the EKS cluster.

    $ kubectl create namespace keramik-0
    $ ./k8s/ceramic/create-secrets.sh
    $ kubectl apply -k ./k8s/ceramic/        # Start up ceramic cluster
    $ kubectl apply -k ./k8s/opentelemetry/  # Start up monitoring infra


## Change network size

The network size can be increase by changing the number of replicas for the ceramic statefulset.


## Runner

The `runner` is a utility for running various jobs to initialize the network and run workloads against it.
Any changes to the runner require that you rebuild it and load it into kind again.

    docker build -t keramik/runner:dev runner/
    kind load docker-image keramik/runner:dev

## Opentelemetry

Add opentelemetry collector to the k8s cluster

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

## Contributing

Contributions are welcome! Opening an issue to disucss your idea is a good first step.
When you are ready please use [convential commit](https://www.conventionalcommits.org/en/v1.0.0/)  messages in your commits and PR titles.
