FROM rust:1.95-bullseye AS builder
WORKDIR /app
COPY . .

RUN \
  --mount=type=cache,target=/app/target/ \
  --mount=type=cache,target=/usr/local/cargo/registry/ \
  cargo build --release && \
  cp ./target/release/api /

FROM debian:bullseye-slim AS runtime
WORKDIR /app
ENV RUST_LOG="api=info,tower_http=info,axum::rejection=trace"
RUN apt update && apt-get install -y ca-certificates
COPY --from=builder /api /usr/local/bin
ENTRYPOINT ["/usr/local/bin/api"]
EXPOSE 1337/tcp