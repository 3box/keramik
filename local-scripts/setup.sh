#!/bin/bash
set -e

# USAGE: ./setup.sh [network.yaml]
# If you don't specify a network, it will use the small.yaml network file in this folder.
# This script will create a kind cluster, install the operator, and load the network into the cluster.
# It will pull all the images to your machine, and then load them into the cluster. This can take a while,
# but it makes subsequent restarts of the cluster much faster, as we don't need to redownload all the images,
# we just need to load them in.

NETWORK=$1
if [ -z "$NETWORK" ]; then
    NETWORK="./small.yaml"
fi
cd "$(dirname "${BASH_SOURCE[0]}")"
# cluster might already exist and that's okay
kind create cluster --config "./kind.yaml" || true

cargo run --bin crdgen --manifest-path "./../Cargo.toml" | kubectl apply -f -

kubectl create namespace keramik || true # same for namespace. we don't care if it already exists
kubectl apply -k "./../k8s/operator/"

python3 ./load.py

kubectl apply -f $NETWORK
NETWORK_NAME=small
echo export NETWORK_NAME=small
kubectl describe network $NETWORK_NAME
kubectl config set-context --current --namespace=keramik-$NETWORK_NAME

