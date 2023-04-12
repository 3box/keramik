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
kind create cluster --name ceramic
kubectl create ns ceramic
# Build the runner image and load it into kind
docker build -t keramik/runner:dev runner/
kind load docker-image keramik/runner:dev
# Create new random secrets
./k8s/create-secrets.sh
# Start up the network
kubectl apply -k ./k8s
```

View logs

```
kubectl logs ceramic-0 -c ceramic
```

## Change network size

The network size can be increase by changing the number of replicas for the ceramic statefulset.


## Runner

The `runner` is a utility for running various jobs to initialize the network and run workloads against it.
Any changes to the runner require that you rebuild it and load it into kind again.

    docker build -t keramik/runner:dev runner/
    kind load docker-image keramik/runner:dev
