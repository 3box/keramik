# Setting Up a Network

With the operator running we can now define a Ceramic network.

Place the following network definition into the file `small.yaml`.

```yaml
# small.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: <unique-name>-small
spec:
  replicas: 2
  # Required if you plan to run a simulation
  monitoring:
    namespaced: true
```

The `<unique-name>` can be any unique string, your initials are a good default if you are deploying the network to a cloud cluster.

Apply this network definition to the k8s cluster:

```shell
kubectl apply -f small.yaml
```

After a minute or two you should have a functioning Ceramic network.

## Checking the status of the network

Check the status of the network:

```shell
export NETWORK_NAME=<unique-name>-small
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

>HINT: Use tools like  [k9s](https://k9scli.io/) to interactively manage your network.

When your pods are ready, you can [run a simulation](./simulation.md).
If you are running locally, be patient as the first time you setup a network you will need to download several images.

>HINT: Use tools like [kubectx](https://github.com/ahmetb/kubectx) or [kubie](https://github.com/sbstp/kubie) to work with multiple namespaces and contexts.

When you're finished, you can tear down your network with the following command:

```shell
kubectl delete network $NETWORK_NAME
```

