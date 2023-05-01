FROM public.ecr.aws/r5b3e0r5/3box/rust-builder:latest as chef

# Goose Cargo dep requires cmake to build
USER root
RUN apt-get update && apt-get install -y \
    cmake \
    && rm -rf /var/lib/apt/lists/*

USER builder

RUN mkdir -p /home/builder/keramik/runner
WORKDIR /home/builder/keramik/runner

# TODO add to parent builder image
RUN cargo install cargo-chef

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /home/builder/keramik/runner/recipe.json recipe.json

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release

FROM ubuntu:latest

COPY --from=builder /home/builder/keramik/runner/target/release/runner /usr/bin

ENTRYPOINT ["/usr/bin/runner"]
