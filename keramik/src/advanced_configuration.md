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
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  ceramic:
    privateKeySecret: "small"
    networkType: "dev-unstable"
    pubsubTopic: ""
    ethRpcUrl: ""
    casApiUrl: "https://some-anchor-service.com"
```