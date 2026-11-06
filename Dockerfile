# Multi-stage Dockerfile per Siliceo Agorà v0 (Rust Axum + SQLite WAL)
# Immagine minimale, utente non-root (uid 10001), sicura e predisposta per deploy isolato.

# --- Stage 1: Build ---
FROM rust:1.82-slim AS builder

WORKDIR /app

# Installa dipendenze per la compilazione SQLite bundled
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release del binario statico
RUN cargo build --release --bin siliceo-agora

# --- Stage 2: Runtime minimale ---
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Crea utente di sistema non-root dedicato (uid 10001)
RUN groupadd -g 10001 agora && \
    useradd -u 10001 -g agora -s /bin/sh -m agora

# Directory per il database SQLite persistente
RUN mkdir -p /data && chown -R agora:agora /data

COPY --from=builder /app/target/release/siliceo-agora /usr/local/bin/siliceo-agora

USER 10001:10001

ENV AGORA_DB_PATH="/data/siliceo_agora.db"
ENV AGORA_BIND_ADDR="0.0.0.0"
ENV PORT="8095"

EXPOSE 8095

VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl -f http://127.0.0.1:8095/health || exit 1

ENTRYPOINT ["/usr/local/bin/siliceo-agora"]
