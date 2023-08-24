# Specifying a Ceramic admin secret

You can choose to specify a private key for the Ceramic nodes to use as their admin secret. This will allow you to set
up the corresponding DID with CAS Auth.

Leaving the private key unspecified will cause a new key to be randomly generated. This can be fine for simulation runs
against CAS/Ganache running locally within the cluster but not for simulations that hit CAS running behind the AWS API
Gateway. Using an unauthorized DID in that case will prevent the Ceramic nodes from starting up.

```yaml
apiVersion: "keramik.3box.io/v1alpha1"
kind: Network
metadata:
  name: small
spec:
  replicas: 2
  privateKeySecret: "small"
```

Note that `privateKeySecret` is the name of another k8s secret in the `keramik` namespace that has already been
populated beforehand with the desired hex-encoded private key. This source secret MUST exist before it can be used to
populate the Ceramic admin secret.

```shell
kubectl create secret generic small --from-literal=private-key=0e3b57bb4d269b6707019f75fe82fe06b1180dd762f183e96cab634e38d6e57b
```

The secret can also be created from a file containing the private key.

```shell
kubectl create secret generic small --from-file=private-key=./my_secret
```

Here's an example of the contents of the `my_secret` file. Please make sure that there are no newlines at the end of
the file.

```
0e3b57bb4d269b6707019f75fe82fe06b1180dd762f183e96cab634e38d6e57b
```

Alternatively, you can use a `kustomization.yml` file to create the secret from a file before creating the network, and
using the name of the new secret in the network configuration.

```yaml
---
namespace: keramik

secretGenerator:
- name: small
  envs:
  - .env.secret
```

Here's an example of the contents of the `.env.secret` file.

```
private-key=0e3b57bb4d269b6707019f75fe82fe06b1180dd762f183e96cab634e38d6e57b
```
