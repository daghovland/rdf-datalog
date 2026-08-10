# Mirrors the repo root Dockerfile's builder/runtime split (see that file's
# comments) for the standalone backlog_endpoint binary -- see #381 Stage 1
# restructuring / backlog_endpoint/README.md for why this is a separate
# crate/binary/image from dagalog's own.
# ── builder ───────────────────────────────────────────────────────────────────
FROM rust:1-slim AS builder
WORKDIR /build
COPY . .
RUN apt-get update && apt-get install -y ca-certificates
RUN cargo build --release -p backlog-endpoint --bin backlog-endpoint

# ── runtime ───────────────────────────────────────────────────────────────────
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/backlog-endpoint /usr/local/bin/backlog-endpoint
EXPOSE 3031
ENTRYPOINT ["backlog-endpoint"]
