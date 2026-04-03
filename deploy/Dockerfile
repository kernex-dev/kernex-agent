# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from source
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked 2>/dev/null; rm -f src/main.rs

# Build actual source (builtins/ is embedded via include_str! at compile time)
COPY src ./src
COPY builtins ./builtins
RUN touch src/main.rs && cargo build --release --locked

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Runtime deps only: TLS certs + curl for healthcheck
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Non-root user — UID/GID 1001
RUN groupadd --gid 1001 kx \
    && useradd --uid 1001 --gid 1001 --no-create-home --shell /bin/false kx \
    && mkdir -p /home/kx/.kx /home/kx/.claude \
    && chown -R kx:kx /home/kx

COPY --from=builder --chown=kx:kx /build/target/release/kx /usr/local/bin/kx

# Entrypoint bootstrap script — populates data volume with defaults on first run
COPY deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Bundle default skills and workflows into the image
# entrypoint.sh copies these to /home/kx/.kx/ on first start
COPY deploy/skills /usr/share/kx/defaults/skills/
COPY deploy/workflows /usr/share/kx/defaults/workflows/

USER kx
WORKDIR /home/kx
ENV HOME=/home/kx

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8080/health || exit 1

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["serve", "--host", "0.0.0.0", "--port", "8080"]
