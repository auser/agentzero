# ── Builder ──────────────────────────────────────────────────────────
FROM rust:1.80-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config make && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY bin/ bin/
COPY crates/ crates/

# Build the server variant (headless: no TUI, no interactive)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build -p agentzero --profile release-min \
      --no-default-features \
      --features memory-sqlite,plugins,gateway,tls-rustls && \
    cp target/release-min/agentzero /usr/local/bin/agentzero

# ── Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r agentzero && useradd -r -g agentzero -m agentzero

COPY --from=builder /usr/local/bin/agentzero /usr/local/bin/agentzero

RUN mkdir -p /data && chown agentzero:agentzero /data

# Embed a minimal default config that allows public binding (required for Docker networking)
RUN printf '[gateway]\nhost = "0.0.0.0"\nport = 8080\nallow_public_bind = true\n' \
    > /data/agentzero-default.toml && \
    chown agentzero:agentzero /data/agentzero-default.toml

ENV AGENTZERO_DATA_DIR=/data
ENV AGENTZERO_CONFIG=/data/agentzero-default.toml

USER agentzero
WORKDIR /data

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["agentzero"]
CMD ["gateway", "--host", "0.0.0.0", "--port", "8080"]
