#!/bin/bash

# Build and tag Docker image
# docker buildx build --load -t samika98/runner:dev1 --target runner --platform linux/amd64 .

# # Push Docker image
# docker push samika98/runner:dev1

# Delete the existing Keramik network
kubectl delete networks.keramik.3box.io keramik-samika-perftest

# Wait for 2 minutes
echo "Waiting for 1 minutes..."
sleep 60

# Apply the first configuration file
kubectl apply -f 1

# Check if steps 6 and 7 should be skipped
# if [ "$1" != "ss" ]; then
#     echo "Waiting for the network to come up (approximately 5 minutes)..."
#     # Wait for about 5 minutes for the network to come up
#     sleep 300

#     # Apply the ceramic-anchoring-benchmark configuration
#     kubectl apply -f ceramic-anchoring-benchmark.yaml
# else
#     echo "Skipping steps 6 and 7 as requested."
# fi
