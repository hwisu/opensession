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

RUN cargo build --release --bin opensession-server

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

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/opensession-server /usr/local/bin/
COPY --from=frontend /build/opensession/web/build /var/www/opensession
COPY migrations/ /var/www/migrations/

ENV OPENSESSION_DATA_DIR=/data
ENV OPENSESSION_WEB_DIR=/var/www/opensession

EXPOSE 3000

VOLUME ["/data"]

CMD ["opensession-server"]
