# syntax = docker/dockerfile:1

FROM lukemathwalker/cargo-chef:latest-rust-1.76-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.lock .
COPY Cargo.toml .
COPY src src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.lock .
COPY Cargo.toml .
COPY src src
RUN cargo build --release

FROM debian:stable-slim AS runtime

RUN apt-get update && \
    apt-get install -y ca-certificates

COPY --from=builder /app/target/release/spam-musubi /usr/local/bin

CMD ["spam-musubi"]
