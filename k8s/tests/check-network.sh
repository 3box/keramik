#!/bin/bash

# Proxy to the k8s API
kubectl proxy --port=8080&
sleep 3

status() {
  curl -s "http://localhost:8080/apis/keramik.3box.io/v1alpha1/networks/migration-tests/status"
}

check_status() {
  status | jq -r 'if (.status != "Failure") and (.status.readyReplicas == .status.replicas) then true else false end'
}

spec_peers() {
  status | jq -r '.status.replicas'
}

available_peers() {
  jq -r '. | length' < /peers/peers.json
}

check_peers() {
  # Wait for 5 minutes, or till the peers list is ready.
  n=0
  until [ "$n" -ge 30 ];
    do
      if [ "$(available_peers)" == "$(spec_peers)" ]; then
        echo Network peers available
        mkdir /config/env
        CERAMIC_URLS=$(jq -j '[.[].ceramic.ceramicAddr] | join(",")' < /peers/peers.json)
        echo "CERAMIC_URLS=$CERAMIC_URLS" > /config/.env
        exit 0
      else
        echo Waiting for network peers...
        sleep 10
        n=$((n+1))
      fi
  done

  echo Network peers unavailable
  exit 1
}

check_network() {
  # Wait for 5 minutes, or till the network is ready.
  n=0
  until [ "$n" -ge 30 ];
    do
      if [ "$(check_status)" == "true" ]; then
        echo Network is ready
        check_peers
      else
        echo Waiting for network ready...
        sleep 10
        n=$((n+1))
      fi
  done

  echo Network failed to start
  exit 1
}

check_network
