# Advanced CAS and Ceramic Configuration

By default, Keramik will instantiate all the resources required for a functional CAS service, including a Ganache
blockchain.

You can configure the Ceramic nodes to use an external instance of the CAS instead of one inside the cluster. If using a
CAS running in 3Box Labs infrastructure, you will also need to specify the Ceramic network type associated with the
node, e.g. `dev-unstable`.

In this case, the Ceramic network PubSub topic must be specified as an empty string in order to clear it from the
Ceramic configuration. Ceramic nodes do not permit the PubSub topic to be specified for a network type that is not one
of `local` or `inmemory`.

You may also specify an Ethereum RPC endpoint for the Ceramic nodes to be able to verify anchors, or set it to an empty
string to clear it from the Ceramic configuration. In the latter case, the Ceramic nodes will come up but will not be
able to verify anchors.

If left unspecified, `networkType` will default to `local`, `pubsubTopic` to `/ceramic/local-keramik`, `ethRpcUrl` to
`http://ganache:8545`, and `casApiUrl` to `http://cas:8081`. These defaults point to an internal CAS using a local
pubsub topic in a fully isolated network.

```yaml
# network configuration
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  privateKeySecret: "small"
  networkType: "dev-unstable"
  pubsubTopic: ""
  ethRpcUrl: ""
  casApiUrl: "https://some-anchor-service.com"
```

# Disabling AWS Functionality
Certain functionality in CAS depends on AWS services. If you are running Keramik in a non-AWS environment, you can
disable this by editing the statefulset for CAS

    kubectl edit statefulsets cas

and adding the following environment variables to the `spec/template/spec/containers/env` config:

```yaml
- name: SQS_QUEUE_URL
  value: ""
- name: MERKLE_CAR_STORAGE_MODE
  value: disabled
```

*Note* statefulsets must be edited every time the network is recreated.

# Image Resources
You can also use the [network](./setup_network.md) specification to specify resources for the pods that are running

```yaml
# network configuration
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  ceramic:
    - resourceLimits:
        cpu: "4"
        memory: "8Gi"
        storage: "2Gi"
```

The above yaml will provide each ceramic pod with 4 cpu cores, 8GB of memory, and 2GB of storage. Dependent on the system you 
are running on you may run out of resources. You can check your resource usage with

```shell
kubectl describe nodes
```

You can also set resources for IPFS within ceramic similarly

```yaml
# network configuration
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  ceramic:
    - ipfs:
       go:
         resourceLimits:
           cpu: "4"
           memory: "8Gi"
           storage: "2Gi"
```

Setting resources for CAS is slightly different, using `casResourceLimits` to set CAS resources

```yaml
# network configuration
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  cas:
    image: ceramicnetwork/ceramic-anchor-service:latest
    casResourceLimits:
      cpu: "250m"
      memory: "1Gi"
```
