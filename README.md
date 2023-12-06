# Keramik

Keramik is a Kubernetes operator for simulating Ceramic networks.

The `k8s` directory contains the kubernetes manifests for deploying Keramik.

## Getting Started

See the [online documentation](https://3box.github.io/keramik/), or follow the steps below to build the docs.

### Local Documentation

Documentation is done using [mdbook](https://rust-lang.github.io/mdBook/guide/installation.html)

    cargo install mdbook

Book source can then be found in [keramik](./keramik) and can be viewed locally with

    make book-serve

## Contributing

Contributions are welcome! Opening an issue to discuss your idea is a good first step.
When you are ready please use [conventional commit](https://www.conventionalcommits.org/en/v1.0.0/)  messages in your commits and PR titles.

