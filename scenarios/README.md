# Custom cluster testing

In order to use these scenarios follow the docbook, also set up your gcloud cluster as follows:

## Once your cluster is set up

```
kc config set-context --current --namespace=[descriptive-name-with-version]

kc apply -f network-with-cas.yaml    # defines the ceramic version

# optionally, with delay - must have istio in your test cluster
kc label namespace [your namespace] istio-injection=enabled
kc apply -f delay-cas.yaml

kc apply -f write-only.yaml  # runs the simulation

```

To see the results, go to https://us3.datadoghq.com/apm/home

Datadog -> APM-> (pick name) -> click -> service overview


## But first, one time, set up your testing cluster

```
gcloud config set project box-benchmarking-ipfs-testing

gcloud config set compute/zone us-central1-c

gcloud container node-pools create e2-standard-4 --cluster load-testing-golda \
 --machine-type=e2-standard-4 --num-nodes=3

# one time get credentials into kubectl
gcloud container clusters get-credentials load-testing-golda

# and datadog creds
kubectl create secret generic datadog-secret --from-literal api-key=695ce3ba-d73c-41f6-9a97-6ea9ddb662f2 --from-literal app-key=9757fdf4ff254ef93caaa0db71c7c215ec711e38

# install istio virtual network overlay
istioctl install --set profile=demo

```
