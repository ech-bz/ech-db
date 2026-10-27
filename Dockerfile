FROM docker.io/lukemathwalker/cargo-chef:0.1.77-rust-1.94.1-slim-bookworm AS chef

WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        curl ca-certificates clang libclang-dev \
    && curl -fsSL -o /tmp/fdb-clients.deb "https://github.com/apple/foundationdb/releases/download/7.3.63/foundationdb-clients_7.3.63-1_$(if [ "$TARGETARCH" = "arm64" ]; then echo aarch64; else echo amd64; fi).deb" \
    && dpkg -i /tmp/fdb-clients.deb \
    && rm /tmp/fdb-clients.deb \
    && rm -rf /var/lib/apt/lists/*
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p ech-db-server
COPY . .
RUN cargo build --release -p ech-db-server

FROM docker.io/library/debian:bookworm-slim
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && curl -fsSL -o /tmp/fdb-clients.deb "https://github.com/apple/foundationdb/releases/download/7.3.63/foundationdb-clients_7.3.63-1_$(if [ "$TARGETARCH" = "arm64" ]; then echo aarch64; else echo amd64; fi).deb" \
    && dpkg -i /tmp/fdb-clients.deb \
    && rm /tmp/fdb-clients.deb \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/ech-db-server /usr/local/bin/ech-db-server

ENTRYPOINT ["/usr/local/bin/ech-db-server"]
