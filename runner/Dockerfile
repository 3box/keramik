FROM public.ecr.aws/r5b3e0r5/3box/rust-builder:latest as chef

RUN mkdir -p /home/builder/keramik/runner
WORKDIR /home/builder/keramik/runner

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /home/builder/keramik/runner/recipe.json recipe.json

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --locked

FROM ubuntu:latest

COPY --from=builder /home/builder/keramik/runner/target/release/runner /usr/bin

ENTRYPOINT ["/usr/bin/runner"]
