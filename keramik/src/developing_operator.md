# Operator

The `operator` automates creating and manipulating networks via custom resource definition.
Any changes to the operator require that you rebuild it and load it into kind again.

```shell
docker buildx build --load -t keramik/operator:dev --target operator .
kind load docker-image keramik/operator:dev
````

Now we need to update the k8s operator definition to use our new image:

Edit `./k8s/operator/kustomization.yaml` to use the `dev` tag

```yaml
images:
  - name: keramik/operator
    newTag: dev
```

Edit `./k8s/operator/manifests/operator.yaml` to use `IfNotPresent` for the `imagePullPolicy`.

```yaml
# ...
      containers:
      - name: keramik-operator
        image: "keramik/operator"
        imagePullPolicy: IfNotPresent # Should be IfNotPresent when using imageTag: dev, but Always if using imageTag: latest
        command:
          - "/usr/bin/keramik-operator"
          # you can use json logs like so
          # - "--log-format"
          # - "json"
          - "daemon"
# ...
```

Update the CRD definitions and apply the Keramik operator:

```shell
cargo run --bin crdgen | kubectl apply -f -
kubectl apply -k ./k8s/operator/
```

See the [operator background](./operator.md) for details on certain design patterns of the operator.

