#!/bin/bash

set -eo pipefail

export CERAMIC_ADMIN_DID=$(composedb did:from-private-key ${CERAMIC_ADMIN_PRIVATE_KEY})

CERAMIC_ADMIN_DID=$CERAMIC_ADMIN_DID envsubst < /ceramic-init/daemon-config.json > /config/daemon-config.json
CERAMIC_ADMIN_PRIVATE_KEY=$CERAMIC_ADMIN_PRIVATE_KEY envsubst < /ceramic-init/daemon-config.json > /config/daemon-config.json
