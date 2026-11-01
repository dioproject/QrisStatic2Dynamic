FROM rust:1.85-slim AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

RUN cargo build --release --locked

# Runtime minimal stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

RUN useradd -r -u 10001 qris && mkdir -p /data && chown -R qris:qris /data

COPY --from=builder /app/target/release/qris-api /usr/local/bin/qris-api

USER qris

ENV APP_BIND=0.0.0.0:8080 \
    DATABASE_URL=sqlite:///data/qris.db \
    RUST_LOG=info

VOLUME ["/data"]

EXPOSE 8080

ENTRYPOINT ["qris-api"]
