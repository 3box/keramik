# Mixed Networks

It is possible to configure multiple sets of Ceramic nodes that different from one another.
For example a network where half of the nodes are running a different version of js-ceramic or IPFS.

## Examples

### Mixed IPFS

The following config creates a network with half of the nodes running Rust based IPFS and the other half Go.


```yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: mixed
spec:
  replicas: 5
  ceramic:
    - ipfs:
        rust: {}
    - ipfs:
        go: {}
```

### Mixed js-ceramic

The following config creates a network with half of the nodes running a `dev-0` of js-ceramic and the other half `dev-1`.


```yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: mixed
spec:
  replicas: 5
  ceramic:
    - image: ceramicnetwork/composedb:dev-0
    - image: ceramicnetwork/composedb:dev-1
```

## Weights

Weights can be used to determine how many replicas of each Ceramic spec are created.
The total network replicas are spread across each Ceramic spec according to its relative weight.

The default `weight` is `1`.
The simplist way to get exact replica counts is to have the weights sum to the replica count.
Then each Ceramic spec will have a number of replicas equal to its weight.
However it can be tedious to ensure weights always add up to the replica count so this is not required.

The total replicas all Ceramic specs will always sum to the configured replica count.
As such some rounding will be applied to get a good approximation of the relative weights.

### Examples


Create 2/3rd nodes with `dev-0` and 1/3rd with `dev-1`.

```yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: mixed
spec:
  replicas: 3
  ceramic:
    - weight: 2
      image: ceramicnetwork/composedb:dev-0 # 2 replicas
    - image: ceramicnetwork/composedb:dev-1 # 1 replica
```

Create 3/4ths nodes with `dev-0` and 1/4rd with `dev-1`.

```yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: mixed
spec:
  replicas: 24
  ceramic:
    - weight: 3
      image: ceramicnetwork/composedb:dev-0 # 18 replicas
    - image: ceramicnetwork/composedb:dev-1 #  6 replicas
```

Create three different version each having half the previous.
In this case weights do not devide evenly so a close approximation is achived.

```yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: mixed
spec:
  replicas: 16
  ceramic:
    - weight: 4
      image: ceramicnetwork/composedb:dev-0 # 10 replicas
    - weight: 2
      image: ceramicnetwork/composedb:dev-1 # 4 replicas
    - weight: 1
      image: ceramicnetwork/composedb:dev-2 # 2 replicas
```
