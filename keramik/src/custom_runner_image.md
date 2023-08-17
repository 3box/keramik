# Using a custom runner image

## Load the Runner Image

The `runner` is a utility for running various jobs to initialize the network and run workloads against it.
Any changes to the runner require that you rebuild it and load it into kind again.

```shell
docker buildx build --load -t keramik/runner:dev --target runner .
kind load docker-image keramik/runner:dev
```

## Setup network with Runner Image

To use a custom runner image when you [setup your network](./setup_network.md), you will need to adjust the yaml you
use to specify how to bootstrap the runner.

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

## Setup simulation with Runner Image

You will also need to specify the image in your [simulation](./simulation.md) yaml.

```yaml
# Custom runner
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
  image: keramik/runner:dev
  imagePullPolicy: IfNotPresent
```