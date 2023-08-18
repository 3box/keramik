# Custom cluster testing

In order to use these scenarios follow the docbook, also set up your gcloud cluster as follows:

## Once your cluster is set up

```
kc config set-context --current --namespace=keramik

# edit the network-with-cas.yaml to specify the desired image
# edit the meta tag accordingly
# add Datadog API key that you do not check in
kc apply -f network-with-cas.yaml    # defines the ceramic version

kc config set-context --current --namespace=keramic-[your label]

kc edit statefulsets cas

####### add 
      - name: SQS_QUEUE_URL
        value: ""
      - name: MERKLE_CAR_STORAGE_MODE
        value: disabled
###########

kc label namespace keramik-[your label] istio-injection=enabled

kc apply -f delay-cas.yaml

# edit write-only.yaml to match the namespace
kc apply -f write-only.yaml  # runs the simulation

```

To see the results, go to https://us3.datadoghq.com/apm/home

Datadog -> APM-> (pick name) -> click -> service overview


## But first, one time, set up your testing cluster

```
gcloud config set project box-benchmarking-ipfs-testing

gcloud config set compute/zone us-central1-c

gcloud container clusters create [your cluster]

gcloud container node-pools create e2-standard-4 --cluster [your cluster] \
 --machine-type=e2-standard-4 --num-nodes=3

# one time get credentials into kubectl
gcloud container clusters get-credentials [your cluster]

# and datadog creds
kubectl create secret generic datadog-secret --from-literal api-key=[API_KEY] --from-literal app-key=[APP_KEY]

# install istio virtual network overlay
istioctl install --set profile=demo

# set up namespace for datadog
kubectl create ns datadog-operator
helm install -n datadog-operator datadog-operator datadog/datadog-operator
kubectl create secret generic datadog-secret --from-literal=api-key=<YOUR APIP-KEY> \
  --from-literal=app-key=<YOUR APP-KEY> -n datadog-operator

kubectl apply -f datadogAgent.yaml  -n datadog-operator

# set up keramik
kubectl create ns keramik
cargo run --bin crdgen | kubectl create -f -
kubectl apply -k k8s/operator/

```
