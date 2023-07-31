# Deploying Images
There are two images that need to be deployed to the cluster if using a [local environment](./environment.md#local-environment)
or if you are trying to develop functionality or [scenarios](./developing-scenarios.md). These images are
 * [Operator](#operator) - long lived process that manages the network custom resource.
 * [Runner](#runner) - short lived process that performs various tasks within the network (i.e. bootstrapping)

## Operator

The `operator` automates creating and manipulating networks via custom resource definition.
Any changes to the operator require that you rebuild it and load it into kind again.

```shell
docker buildx build --load -t keramik/operator:dev --target operator .
kind load docker-image keramik/operator:dev
````

Next edit `./k8s/operator/kustomization.yaml` to use the `dev` tag

```yaml
images:
  - name: keramik/operator
    newTag: dev
```

Finally apply these changes:

```shell
kubectl apply -k ./k8s/operator/
```

See the [operator background](./operator.md) for details on certain design patterns of the operator.

## Runner

The `runner` is a utility for running various jobs to initialize the network and run workloads against it.
Any changes to the runner require that you rebuild it and load it into kind again.

```shell
docker buildx build --load -t keramik/runner:dev --target runner .
kind load docker-image keramik/runner:dev
```

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

You will then apply this to start the runner

```shell
kubectl apply -f small.yaml
```
