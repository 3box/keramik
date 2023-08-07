# Datadog

Keramik can also be configured to send metrics and telemetry data to datadog.

You will first need to [setup a barebones network](setup_network.md) that we can install the datadog operator
into. An example barebones network from the above setup:

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: <name of network>
spec:
  replicas: 1
  datadog:
    enabled: true
    version: "unique_value"
    profilingEnabled: true
```

You will need to install the datadog k8s operator into the network. This requires
[installing](https://helm.sh/docs/intro/install/) `helm`, there doesn't seem to be any other way to install the operator
without first installing helm. However once the datadog operator is installed helm is no longer needed.

    helm repo add datadog https://helm.datadoghq.com
    helm install my-datadog-operator datadog/datadog-operator

Now we will use that barebones network to setup secrets for datadog, and the datadog agent. Adjust the previously 
defined network definition to look like the following:

```yaml
# Network setup
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  datadog:
    enabled: true
    version: "unique_value"
    profilingEnabled: true
    
# Secrets Setup
---
apiVersion: v1
kind: Secret
metadata:
  name: datadog-secret
type: Opaque
stringData:
  api-key: <Datadog API Key Secret>
  app-key: <Datadog Application Key Secret>
    
# Datadog Agent setup
---
kind: DatadogAgent
apiVersion: datadoghq.com/v2alpha1
metadata:
  name: datadog
spec:
  global:
    kubelet:
      tlsVerify: false
    site: us3.datadoghq.com
    credentials:
      apiSecret:
        secretName: datadog-secret
        keyName: api-key
      appSecret:
        secretName: datadog-secret
        keyName: app-key
  override:
    clusterAgent:
      image:
        name: gcr.io/datadoghq/cluster-agent:latest
    nodeAgent:
      image:
        name: gcr.io/datadoghq/agent:latest
  features:
    npm:
      enabled: true
    apm:
      enabled: true
      hostPortConfig:
        enabled: true
```

The Datadog API Key is found at the organization level, and should be the secret associated with the API Key. The 
Datadog application key can be found at the organization or user level, and should be the secret associated with the
application key.

You can now apply this with

    kubectl apply -f network.yaml

*Note* If you are running locally, you will need to restart your CAS and Ceramic pods using

    kubectl delete pod ceramic-0 ceramic-1 cas-0

where the ceramic pods will depend on the replicas used. Make sure you delete all Ceramic and CAS pods. This only needs 
to be done the 

Anytime you need to change the network, change this file, then reapply it with

    kubectl apply -f network.yaml

Telemetry data sent to datadog will have two properties to uniquely identifiy the data from other keramik networks.

* `env` - this is set based on the namespace of the keramik network.
* `version` - specified in the datadog config, may be any unique value.

## Cleanup

    kubectl delete -f network.yaml
    helm delete my-datadog-operator


