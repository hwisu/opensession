# ── Stage 1: Rust build ──────────────────────────────────────────────────────
FROM rust:1.85-bookworm AS builder

WORKDIR /app

# Copy opensession-core workspace (via additional_contexts)
COPY --from=opensession-core Cargo.toml /opensession-core/Cargo.toml
COPY --from=opensession-core crates/ /opensession-core/crates/

COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/

# Override git deps with local paths for Docker build
RUN mkdir -p .cargo && printf '[patch."https://github.com/hwisu/opensession-core"]\nopensession-core = { path = "/opensession-core/crates/core" }\nopensession-parsers = { path = "/opensession-core/crates/parsers" }\nopensession-api-types = { path = "/opensession-core/crates/api-types" }\n' > .cargo/config.toml

RUN cargo build --release --bin opensession-server && \
    strip /app/target/release/opensession-server

# ── Stage 2: Frontend build ──────────────────────────────────────────────────
FROM node:22-slim AS frontend

WORKDIR /build/opensession

# opensession-core packages/ui (via additional_contexts)
COPY --from=opensession-core packages/ui/ /build/opensession-core/packages/ui/
RUN cd /build/opensession-core/packages/ui && npm install

COPY web/package.json web/package-lock.json web/
RUN cd web && npm ci

COPY web/ web/
RUN cd web && npm run build

# ── Stage 3: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

LABEL org.opencontainers.image.title="OpenSession"
LABEL org.opencontainers.image.source="https://github.com/hwisu/opensession"
LABEL org.opencontainers.image.licenses="MIT"

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl tini && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r opensession && useradd -r -g opensession -s /bin/false opensession

RUN mkdir -p /data && chown opensession:opensession /data

COPY --from=builder --chown=opensession:opensession /app/target/release/opensession-server /usr/local/bin/
COPY --from=frontend --chown=opensession:opensession /build/opensession/web/build /var/www/opensession
COPY --chown=opensession:opensession migrations/ /var/www/migrations/

ENV OPENSESSION_DATA_DIR=/data
ENV OPENSESSION_WEB_DIR=/var/www/opensession

EXPOSE 3000

VOLUME ["/data"]

USER opensession

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -f http://localhost:3000/api/health || exit 1

ENTRYPOINT ["tini", "--"]
CMD ["opensession-server"]
