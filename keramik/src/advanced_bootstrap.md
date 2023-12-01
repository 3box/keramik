# Advanced Bootstrap Configuration

## Disable Bootstrap

By default, Keramik will connect all IPFS peers to each other.
This can be disabled using specific bootstrap configuration:


```yaml
# network configuration
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  bootstrap:
    enabled: false
```
