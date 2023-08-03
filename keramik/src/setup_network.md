# Setting Up a Network
With the operator running we can now define a Ceramic network.

Place the following network definition into the file `small.yaml`. You can also use a [custom image](./custom_runner_image.md)
for the runner when setting up a network.

```yaml
# small.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: <initials>-small
spec:
  replicas: 2
```

If you're running a simulation in the cloud, you will want to use `version` to separate out the metrics and errors for
your simulation run.

```yaml
datadog:
  enabled: true
  version: "unique-simulation-name"
  profilingEnabled: true
```

A complete example of a network definition with datadog enabled can be found [here](./datadog.md).

Apply this network definition to the k8s cluster:

```shell
kubectl apply -f small.yaml
```

After a minute or two you should have a functioning Ceramic network.

## Checking the status of the network
Check the status of the network:

```shell
export NETWORK_NAME=<initials>-small
kubectl describe network $NETWORK_NAME
```

Keramik places each network into its own namespace named after the name of the network. You can default your context
to this namespace using:

```shell
kubectl config set-context --current --namespace=keramik-$NETWORK_NAME
```

Inspect the pods within the network using:

```shell
kubectl get pods
```

When your pods are ready, you can [run a simulation](./simulation.md)

>HINT: Use tools like [kubectx](https://github.com/ahmetb/kubectx) or [kubie](https://github.com/sbstp/kubie) to work with multiple namespaces and contexts.

When you're finished, you can tear down your network with the following command:

```shell
kubectl delete network $NETWORK_NAME
```

