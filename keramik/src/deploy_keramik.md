# Deploy Keramik

To deploy keramik, we will need to deploy custom resource definitions (CRDs) and apply the Keramik operator.

## Deploy CRDS

Custom resource definitions tell k8s about our network and simulation resources.
When deploying a new cluster and anytime they change you need to apply them.

To generate the CRDs from current code, run the following command:
```shell
cargo run --bin crdgen | kubectl apply -f -
```
To apply a version of the CRDs from a release, run the following command:
```shell
kubectl apply -f k8s/crds/v1alpha1.yaml
``````

## Deploy Keramik  Operator

The last piece to running Keramik is the operator itself. Apply the operator into the `keramik` namespace.

```
# Create keramik namespace
kubectl create namespace keramik
# Apply the keramik operator
kubectl apply -k ./k8s/operator/
```

Once that is complete, you can now [setup a network](./setup_network.md).
