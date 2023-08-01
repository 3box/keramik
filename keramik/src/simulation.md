# Simulation

To run a simulation, first define a simulation.
```yaml
# basic.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Simulation
metadata:
  name: basic
  namespace: keramik-small
spec:
  scenario: ceramic-simple
  users: 10
  run_time: 4
```
If you want to run it against a defined network, set the namespace to the same as the network. in this example the 
namespace is set to the same network applied when [the network was setup](./setup_network.md).
Additionally, you can define the scenario you want to run, the number of users, and the number of minutes it will run.

Once ready, apply this simulation defintion to the k8s cluster:

```shell
kubectl apply -f basic.yaml
```

Keramik will first start all the metrics and tracing resources, once ready it will start the simulation by first starting the simulation manager and then all the workers.
The manager and workers will stop once the simulation is complete.

You can then [analyze](analysis.md) the results of the simulation.


