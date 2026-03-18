# Plan 01: Containerization (Dockerfile + Compose + CI)

## Problem

No Dockerfile exists anywhere in the repo. The production guide at `site/src/content/docs/guides/production.md` documents a `docker-compose.yml` example inline, but users must build the Rust binary themselves. This is the #1 adoption blocker — most deployment targets (cloud, VPS, k8s, home servers) expect a container image.

Competing tools claim "2-minute setup" — AgentZero requires a full Rust toolchain install and multi-minute compilation. A pre-built container image closes this gap entirely.

## Current State

- Binary builds for 8 targets in `.github/workflows/release.yml` (Linux x86_64/aarch64/armv7/musl variants, macOS, Windows)
- Production guide documents nginx/Caddy TLS termination, Docker Compose example (inline markdown), systemd service
- `GET /health` endpoint exists in `crates/agentzero-gateway/src/handlers.rs:37`
- Daemon mode with PID file, log rotation in `crates/agentzero-cli/src/daemon.rs`
- `just build` produces release binary; `just build-minimal` for stripped-down variant

## Implementation

### 1. Multi-stage Dockerfile (`Dockerfile`)

```dockerfile
# Stage 1: Builder
FROM rust:1.82-bookworm AS builder
ARG FEATURES=default
ARG PROFILE=release
WORKDIR /src
COPY . .
RUN cargo build --profile ${PROFILE} --features ${FEATURES} -p agentzero \
    && cp target/${PROFILE}/agentzero /usr/local/bin/agentzero

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*
RUN groupadd -r agentzero && useradd -r -g agentzero -d /data agentzero
COPY --from=builder /usr/local/bin/agentzero /usr/local/bin/agentzero
USER agentzero
WORKDIR /data
VOLUME ["/data"]
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -f http://localhost:3000/health || exit 1
ENTRYPOINT ["agentzero"]
CMD ["serve"]
```

Key decisions:
- `bookworm-slim` (not alpine) because SQLCipher/OpenSSL are easier on glibc
- Build arg `FEATURES` lets users build minimal or privacy variants
- Non-root user (UID auto-assigned, common for security scanners)
- `/data` volume for config, SQLite DB, encryption keys
- `curl` included for healthcheck (small cost, big debugging benefit)

### 2. `.dockerignore`

```
target/
.git/
plugins/*/target/
site/node_modules/
*.md
!README.md
```

### 3. `docker-compose.yml`

```yaml
services:
  agentzero:
    build: .
    ports:
      - "3000:3000"
    volumes:
      - agentzero-data:/data
    environment:
      AGENTZERO__GATEWAY__ENABLED: "true"
      AGENTZERO__GATEWAY__HOST: "0.0.0.0"
      AGENTZERO__GATEWAY__PORT: "3000"
    restart: unless-stopped

volumes:
  agentzero-data:
```

Optional monitoring stack (commented out by default):
- `prometheus` service scraping `:3000/metrics`
- `grafana` service with pre-built dashboard

### 4. CI container pipeline

Add job to `.github/workflows/release.yml`:

```yaml
container:
  needs: [build]
  runs-on: ubuntu-latest
  permissions:
    packages: write
  steps:
    - uses: actions/checkout@v4
    - uses: docker/setup-buildx-action@v3
    - uses: docker/login-action@v3
      with:
        registry: ghcr.io
        username: ${{ github.actor }}
        password: ${{ secrets.GITHUB_TOKEN }}
    - uses: docker/build-push-action@v6
      with:
        push: true
        platforms: linux/amd64,linux/arm64
        tags: |
          ghcr.io/${{ github.repository }}:latest
          ghcr.io/${{ github.repository }}:${{ github.ref_name }}
```

Multi-arch via `docker buildx` (amd64 + arm64 covers 95%+ of deployment targets).

### 5. Justfile recipes

```just
# Build Docker image
docker-build:
    docker build -t agentzero .

# Build minimal Docker image
docker-build-minimal:
    docker build --build-arg FEATURES=minimal -t agentzero:minimal .

# Run with Docker Compose
docker-up:
    docker compose up -d

docker-down:
    docker compose down
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `Dockerfile` | Create |
| `.dockerignore` | Create |
| `docker-compose.yml` | Create |
| `.github/workflows/release.yml` | Add container build job |
| `Justfile` | Add docker recipes |

## Verification

1. `docker build -t agentzero .` succeeds
2. `docker run --rm agentzero --version` prints version
3. `docker compose up -d` starts; `curl localhost:3000/health` returns `{"status":"ok"}`
4. `docker compose down` cleans up
5. Multi-arch build works: `docker buildx build --platform linux/amd64,linux/arm64 .`
6. Image size < 100MB (slim runtime stage)
7. Container runs as non-root (verify with `docker exec ... id`)

## Risks

- SQLCipher bundled compilation may be slow in Docker (mitigate: use cargo cache layer)
- arm64 cross-compilation may require `cargo-zigbuild` in Docker (mitigate: use buildx QEMU or native arm64 runner)
