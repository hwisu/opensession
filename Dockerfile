# ── Stage 1: Rust build ──────────────────────────────────────────────────────
FROM rust:1.83-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/

# Build release binary (fetches git deps from open-session/core)
RUN cargo build --release --bin opensession-server

# ── Stage 2: Frontend build ──────────────────────────────────────────────────
FROM node:22-slim AS frontend

WORKDIR /app

# Install packages/ui dependencies first
COPY packages/ui/package.json packages/ui/package-lock.json packages/ui/
RUN cd packages/ui && npm ci

# Copy packages/ui source
COPY packages/ui/ packages/ui/

# Install web dependencies
COPY web/package.json web/package-lock.json web/
RUN cd web && npm ci

# Copy web source and build
COPY web/ web/
RUN cd web && npm run build

# ── Stage 3: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/opensession-server /usr/local/bin/
COPY --from=frontend /app/web/build /var/www/opensession
COPY migrations/ /var/www/migrations/

ENV OPENSESSION_DATA_DIR=/data
ENV OPENSESSION_WEB_DIR=/var/www/opensession

EXPOSE 3000

VOLUME ["/data"]

CMD ["opensession-server"]
