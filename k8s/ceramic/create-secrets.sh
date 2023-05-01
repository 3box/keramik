kubectl create secret generic postgres-auth \
  --namespace keramik-0 \
  --from-literal=username=ceramic --from-literal=password=$(openssl rand -hex 20)
kubectl create secret generic ceramic-admin \
  --namespace keramik-0 \
  --from-literal=private-key=$(openssl rand -hex 32)
