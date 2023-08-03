# Datadog

Keramik can also be configured to send metrics and telemetry data to datadog.

We will be following these steps.

0. Setup Keramik network
1. Install the operator
2. Configure the operator
3. Configure secrets for the operator
4. Configure datadog within a Keramik network.

The datadog operator needs to be installed into an existing Keramik network.
Create Keramik network using `small.yaml` from [here](setup_network.md).

    kubectl apply -f small.yaml

Install the datadog k8s operator into the network.
This requires [installing](https://helm.sh/docs/intro/install/) `helm`, there doesn't seem to be any other way to install the operator without first installing helm.
However once the datadog operator is installed helm is no longer needed.

    helm repo add datadog https://helm.datadoghq.com
    helm install -n keramik-small my-datadog-operator datadog/datadog-operator

Configure the datadog operator:

```yaml
#dd.yaml
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

Apply this yaml file into the namespace of the network:

    kubectl apply -n keramik-small -f dd.yaml

Setup secrets:

    kubectl create secret generic datadog-secret --from-literal api-key=<DATADOG_API_KEY> --from-literal app-key=<DATADOG_APP_KEY>

Replace `<DATADOG_API_KEY>` and `<DATADOG_APP_KEY>` accordingly.

Enable datadog reporting for the network:

```yaml
# small.yaml
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
```

```shell
kubectl apply -f small.yaml
```

Telemetry data sent to datadog will have two properties to uniquely identifiy the data from other keramik networks.

* `env` - this is set based on the namespace of the keramik network.
* `version` - specified in the datadog config, may be any unique value.

## Cleanup

    kubectl delete datadogagent datadog
    helm delete my-datadog-operator


