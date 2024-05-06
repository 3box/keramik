FROM rust-builder:latest as builder

RUN mkdir -p /home/builder/keramik/
WORKDIR /home/builder/keramik/

# Copy in source code
COPY . .

# Build application using a docker cache
# To clear the cache use:
#   docker builder prune --filter type=exec.cachemount
RUN --mount=type=cache,target=/home/builder/.cargo \
	--mount=type=cache,target=/home/builder/keramik/target \
    make build && \
    cp ./target/release/keramik-runner ./target/release/keramik-operator ./

# This image needs to be the same as the parent image of rust-builder
FROM debian:bookworm-slim as exec

RUN apt-get update && apt-get install -y \
    openssl \
    && rm -rf /var/lib/apt/lists/*

FROM exec as runner

COPY --from=builder /home/builder/keramik/keramik-runner /usr/bin

ENTRYPOINT ["/usr/bin/keramik-runner"]

FROM exec as operator

COPY --from=builder /home/builder/keramik/keramik-operator /usr/bin

ENTRYPOINT ["/usr/bin/keramik-operator"]
