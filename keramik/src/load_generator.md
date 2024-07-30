# Load Generation

To run a load generator, you need to create a `LoadGenerator` resource. This resource is similar to a `Simulation` resource. However, the load generation can last for up to a week. They are used to generate sustained load on the system for a longer period of time.

## Parameters

- **`scenario`**: The scenario to run. Supported scenarios are:
  - `CreateModelInstancesSynced`: Requires at least two ceramic instances. Creates models on one node and has the other node sync them.
- **`runTime`**: The duration to run the load generator, in hours.
- **`image`**: The image to use for the load generator. This is the same as the `image` in the `Simulation` resource.
- **`throttleRequests`**: WIP, not ready yet. The number of requests to send per second. This is the same as the `throttleRequests` in the `Simulation` resource.
- **`tasks`**: The number of tasks to run. Increasing the number of tasks will increase the load on the node. A value of 2 generates a steady load of 20 requests per second. Values between 2-100 are recommended. Keep in mind the increase of tasks to throughput is non-linear. A value of 100 generates what we consider high load, which is 200 TPS.

## Sample configuration

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: LoadGenerator
metadata:
  name: load-gen
  # Namespace of the network you wish to run against
  namespace: keramik-<unique-name>-small
spec:
  scenario: CreateModelInstancesSynced
  runTime: 3
  image: "keramik/runner:dev"
  throttleRequests: 20
  tasks: 2
```

If you want to run this against a defined network, set the namespace to the same as the network. In this example, the namespace is set to the same network applied when [the network was set up](./setup_network.md).

The load generator will automatically stop once the `runTime` is up. You should be able to see some success and error metrics at the end of the run. To see the metrics, you can use the `kubectl` command to get the logs of the load generator:


```shell
kubectl logs load-gen-<unique-string-for-each-run> -n keramik-<unique-name>-small
```
You can get the name of the load-gen pod by running:

```shell
kubectl get pods -n keramik-<unique-name>-small
```