# Using a custom runner image
To use a custom runner image when you [setup your network](./setup_network.md), you will need to adjust the yaml you
use to specify how to bootstrap the runner.

```yaml
# small.yaml
---
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  # Use custom runner image for bootstrapping
  bootstrap:
    image: keramik/runner:dev
    imagePullPolicy: IfNotPresent
```