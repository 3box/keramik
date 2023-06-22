#!/bin/bash

# Build and publish a docker images
#
# Use:
#
#   export DOCKER_PASSWORD=$(aws ecr-public get-login-password --region us-east-1)
#   echo "${DOCKER_PASSWORD}" | docker login --username AWS --password-stdin public.ecr.aws/r5b3e0r5
#
# to setup docker login.

# Build runner image
docker buildx build --load -t 3box/keramik-runner:latest --target runner .
docker tag 3box/keramik-runner:latest public.ecr.aws/r5b3e0r5/3box/keramik-runner:latest
docker push public.ecr.aws/r5b3e0r5/3box/keramik-runner:latest

# Build operator image
docker buildx build --load -t 3box/keramik-operator:latest --target operator .
docker tag 3box/keramik-operator:latest public.ecr.aws/r5b3e0r5/3box/keramik-operator:latest
docker push public.ecr.aws/r5b3e0r5/3box/keramik-operator:latest
