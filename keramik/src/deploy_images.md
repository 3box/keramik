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

If you have already [created a cluster](./create_cluster.md), you will need to apply these changes:

```shell
kubectl apply -k ./k8s/operator/
```

See the [operator background](./operator.md) for details on certain design patterns of the operator.

Next you will [deploy custom resource definitions](./deploy_crds.md) to the cluster. If you've already deployed CRDS and setup a network, you will 
need to [setup a network](./setup_network.md) again to use your new runner.