# ── Builder ──────────────────────────────────────────────────────────
FROM rust:1.86-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config make libssl-dev && \
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
    iptables ca-certificates curl python3 python3-yaml && \
    rm -rf /var/lib/apt/lists/*

# Non-root user (uid 1000)
RUN groupadd -g 1000 agentzero && \
    useradd -u 1000 -g agentzero -m agentzero

COPY --from=builder /usr/local/bin/agentzero /usr/local/bin/agentzero

# Writable directories for the sandbox runtime
RUN mkdir -p /data /sandbox /workspace && \
    chown agentzero:agentzero /data /sandbox && \
    chmod 755 /workspace

# Embed a minimal default config that allows public binding (required for Docker networking)
RUN printf '[gateway]\nhost = "0.0.0.0"\nport = 8080\nallow_public_bind = true\n' \
    > /data/agentzero-default.toml && \
    chown agentzero:agentzero /data/agentzero-default.toml

# Copy entrypoint and policy converter
COPY docker/sandbox/sandbox-entrypoint.sh /usr/local/bin/sandbox-entrypoint.sh
COPY docker/sandbox/policy-to-iptables.py /usr/local/bin/policy-to-iptables.py
RUN chmod +x /usr/local/bin/sandbox-entrypoint.sh /usr/local/bin/policy-to-iptables.py

ENV AGENTZERO_DATA_DIR=/data
ENV AGENTZERO_CONFIG=/data/agentzero-default.toml
ENV AGENTZERO__LOGGING__FORMAT=json

# /workspace is the read-only project mount point
VOLUME ["/workspace"]

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || curl -fk https://localhost:8080/health || exit 1

ENTRYPOINT ["sandbox-entrypoint.sh"]
CMD ["gateway", "--host", "0.0.0.0", "--port", "8080"]
