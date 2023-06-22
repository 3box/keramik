#!/bin/bash

rm simulate-workers.yaml
for i in {0..19}
do
cat >> simulate-workers.yaml << EOF
---
apiVersion: batch/v1
kind: Job
metadata:
  name: simulate-worker-$i
spec:
  template:
    spec:
      containers:
      - name: worker
        image: keramik/runner
        imagePullPolicy: IfNotPresent
        command:
          - "/usr/bin/keramik-runner"
          - "simulate"
        env:
          - name: RUNNER_OTLP_ENDPOINT
            value: 'http://otel:4317'
          - name: RUST_LOG
            value: 'info,keramik_runner=trace'
          - name: RUST_BACKTRACE
            value: '1'
          - name: SIMULATE_SCENARIO
            value: 'ceramic-write-only'
          - name: SIMULATE_TARGET_PEER
            value: '$i'
          - name: SIMULATE_PEERS_PATH
            value: '/keramik-peers/peers.json'
          - name: SIMULATE_NONCE
            value: '61261'
          - name: DID_KEY
            value: 'did:key:z6Mkqn5jbycThHcBtakJZ8fHBQ2oVRQhXQEdQk5ZK2NDtNZA'
          - name: DID_PRIVATE_KEY
            value: '86dce513cf0a37d4acd6d2c2e00fe4b95e0e655ca51e1a890808f5fa6f4fe65a'
        volumeMounts:
          - name: keramik-peers
            mountPath: /keramik-peers
      volumes:
        - name: keramik-peers
          configMap:
            name: "keramik-peers"
            defaultMode: 0755
      restartPolicy: Never
  backoffLimit: 4
EOF

done


