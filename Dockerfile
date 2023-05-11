FROM public.ecr.aws/r5b3e0r5/3box/rust-builder:latest as builder

RUN mkdir -p /home/builder/keramik/
WORKDIR /home/builder/keramik/

# Use the same ids as the parent docker image by default
ARG UID=1001
ARG GID=1001

# Copy in source code
COPY . .

# Build application using a docker cache
# To clear the cache use:
#   docker builder prune --filter type=exec.cachemount
RUN --mount=type=cache,target=/home/builder/.cargo,uid=$UID,gid=$GID \
	--mount=type=cache,target=/home/builder/keramik/target,uid=$UID,gid=$GID \
    make build && \
    cp ./target/release/keramik-runner ./target/release/keramik-operator ./

FROM ubuntu:latest as runner

COPY --from=builder /home/builder/keramik/keramik-runner /usr/bin

ENTRYPOINT ["/usr/bin/keramik-runner"]

FROM ubuntu:latest as operator

COPY --from=builder /home/builder/keramik/keramik-operator /usr/bin

ENTRYPOINT ["/usr/bin/keramik-operator"]
