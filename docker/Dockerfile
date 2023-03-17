FROM lukemathwalker/cargo-chef:latest-rust-1.64.0 AS chef
WORKDIR app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
RUN apt-get update && apt-get install -y libssl-dev pkg-config clang
RUN cargo install bunyan

COPY --from=planner /app/recipe.json recipe.json

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --bin zkbob-cloud

FROM debian:bullseye-slim as runtime

ARG VERSION__REF_NAME
ARG VERSION__COMMIT_HASH
ENV VERSION__REF_NAME=${VERSION__REF_NAME}
ENV VERSION__COMMIT_HASH=${VERSION__COMMIT_HASH}

RUN apt-get update && apt-get install -y libssl-dev

WORKDIR app

COPY --from=builder /app/configuration/base.yaml /app/configuration/base.yaml
COPY --from=builder /app/docker/run.sh /app/run.sh
COPY --from=builder /app/target/release/zkbob-cloud /usr/local/bin
COPY --from=builder /usr/local/cargo/bin/bunyan /usr/local/bin

RUN apt-get update && apt-get install -y ca-certificates && apt-get clean
CMD ["./run.sh"]
