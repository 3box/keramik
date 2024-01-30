# Monitoring

You can enable monitoring on a network to deploy jaeger, prometheus and an opentelemetry collector into the network namespace.
This is not the only way to monitor network resources but it is built in.

Metrics from all pods in the network will be collected.

Sample network resource with monitoring enabled.

```yaml
# basic.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: network-with-monitoring
spec:
  replicas: 2
  monitoring:
    namespaced: true
```

To view the metrics and traces port-forward the services:

    kubectl port-forward prometheus-0 9090
    kubectl port-forward jaeger-0 16686

Then navigate to http://localhost:9090 for metrics and http://localhost:16686 for traces.

## Exposed Metrics

The opentelemetry collector exposes metrics on two different ports under the `otel` service:

* otel:9464 - All metrics collected
* otel:9465 - Only simulation metrics

Simulations will publish specific summary metrics about the simulation run.
This is typically a collection of metrics per simulation run and is much lighter weight than all metrics from all pods in the network.

Scrape the `otel:9465` endpoint if you want on the simulation metrics.

>NOTE: The prometheus-0 pod will scrape all metrics so you can easily inspect all activity on the network.
