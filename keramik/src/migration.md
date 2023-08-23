# Migration Tests

The Rust Ceramic migration tests can be executed against a Keramik network by applying the configuration from
`/k8s/tests`. The network and tests run in the `keramik-migration-tests` namespace but this can be easily changed.

The URLs of the Ceramic nodes in the network are injected into the test environment so that tests are able to hit the
Ceramic API endpoints.

These tests are intended to cover things like Kubo vs. Rust Ceramic API correctness/compatibility, mixed network
operation, longevity tests across updates and releases, etc. Eventually, they can be used to run smoke tests, additional
e2e tests, etc.
