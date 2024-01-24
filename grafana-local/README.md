# grafana local

!!! NOTHING IS PERSISTED !!!  (even admin password)

Update kustomization.yaml to target the correct namespace.

Then deploy with:

```bash
kustomize apply -k ./grafana-local
```

Port forward to access grafana (from your namespace):

```bash
kubectl port-forward svc/grafana 3000:3000
```

Then you can access grafana at:

```
http://localhost:3000
```

Anonymoun log in is allowed.

But if you want to change anything, you need to log in as admin.

Default password is `admin`.

