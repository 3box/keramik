# Deploy Keramik
To deploy keramik, we will need to deploy custom resource definitions (CRDs) and the keramik operator.

```shell
# Create keramik namespace
kubectl create namespace keramik
# Create CRDs
cargo run --bin crdgen | kubectl create -f - 
# Start up keramik operator
kubectl apply -k ./k8s/operator/             
```

Once that is complete, we will [setup a network](./setup_network.md).