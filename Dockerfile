# ── builder ───────────────────────────────────────────────────────────────────
FROM rust:1-slim AS builder
WORKDIR /build
COPY . .
RUN cargo build --release --bin dagalog

# ── runtime ───────────────────────────────────────────────────────────────────
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/dagalog /usr/local/bin/dagalog
EXPOSE 3030
ENTRYPOINT ["dagalog"]
CMD ["--serve"]
