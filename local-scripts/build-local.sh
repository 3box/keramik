#!/bin/bash
set -e

docker buildx build --load -t keramik/operator:dev --target operator .
docker buildx build --load -t keramik/runner:dev --target runner .

kind load docker-image keramik/runner:dev
kind load docker-image keramik/operator:dev