# IPFS

The IPFS behavior used by CAS and Ceramic can be customized using the same IPFS spec.

## Rust IPFS

### Ceramic

Example [network config](./setup_network.md) that uses Rust based IPFS (i.e. ceramic-one) with its defaults for Ceramic.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-ceramic-one
spec:
  replicas: 5
  ceramic:
    - ipfs:
        rust: {}
```

Example [network config](./setup_network.md) that uses Rust based IPFS (i.e. ceramic-one) with a specific image for Ceramic.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-ceramic-one
spec:
  replicas: 5
  ceramic:
    - ipfs:
       rust:
         image: rust-ceramic/ceramic-one:dev
         imagePullPolicy: IfNotPresent
```

### CAS

Example [network config](./setup_network.md) that uses Rust based IPFS (i.e. ceramic-one) with a specific image for CAS.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-ceramic-one
spec:
  replicas: 5
  cas:
    ipfs:
      rust: {}
```

Example [network config](./setup_network.md) that uses Rust based IPFS (i.e. ceramic-one) with a specific image for CAS.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-ceramic-one
spec:
  replicas: 5
  cas:
    ipfs:
     rust:
       image: rust-ceramic/ceramic-one:dev
       imagePullPolicy: IfNotPresent
```

## Kubo IPFS

### Ceramic

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with its defaults for Ceramic.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-kubo
spec:
  replicas: 5
  ceramic:
    - ipfs:
        go: {}
```

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with a specific image for Ceramic.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-kubo
spec:
  replicas: 5
  ceramic:
    - ipfs:
       go:
         image: ceramicnetwork/go-ipfs-daemon:develop
         imagePullPolicy: IfNotPresent
```

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with extra configuration commands for Ceramic.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-kubo
spec:
  replicas: 5
  ceramic:
    - ipfs:
       go:
         image: ceramicnetwork/go-ipfs-daemon:develop
         imagePullPolicy: IfNotPresent
         commands:
           - ipfs config --json Swarm.RelayClient.Enabled false
```

### CAS

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with its defaults for CAS.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-vanilla-kubo
spec:
  replicas: 5
  cas:
    ipfs:
      go: {}
```

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with a specific image for CAS.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-kubo
spec:
  replicas: 5
  cas:
    ipfs:
     go:
       image: ceramicnetwork/go-ipfs-daemon:develop
       imagePullPolicy: IfNotPresent
```

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with extra configuration commands for CAS.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: example-custom-kubo
spec:
  replicas: 5
  cas:
    ipfs:
     go:
       image: ceramicnetwork/go-ipfs-daemon:develop
       imagePullPolicy: IfNotPresent
       commands:
         - ipfs config --json Swarm.RelayClient.Enabled false
```

## Migration from Kubo to Ceramic One

A Kubo blockstore can be migrated to Ceramic One by specifying the migration command in the IPFS configuration.

Example [network config](./setup_network.md) that uses Go based IPFS (i.e. Kubo) with its defaults for Ceramic (including a default
blockstore path of `/data/ipfs`) and the Ceramic network set to `dev-unstable`.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: basic-network
spec:
  replicas: 5
  ceramic:
    - ipfs:
        go: {}
  networkType: dev-unstable
```

Example [network config](./setup_network.md) that uses Ceramic One and specifies what migration command to run before
starting up the node.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: basic-network
spec:
  replicas: 5
  ceramic:
    - ipfs:
        rust:
            migrationCmd:
                - from-ipfs
                - -i
                - /data/ipfs/blocks
                - -o
                - /data/ipfs/
                - --network
                - dev-unstable
```
