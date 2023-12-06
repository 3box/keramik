# Creating a Cluster

Kind (Kubernetes in Docker) runs a local k8s cluster. Create and initialize a new kind cluster using this configuration:

```yaml
# kind.yaml
---
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
featureGates:
  MaxUnavailableStatefulSet: true
```

This configuration enables a feature that allows stateful sets to more rapidly redeploy pods on changes.
While not required to use keramik it makes deploying and mutating networks significantly faster.

```shell
# Create a new kind cluster (i.e. local k8s)
kind create cluster --config kind.yaml
```

Now you will need to [deploy Keramik](./deploy_keramik.md) to the cluster.




