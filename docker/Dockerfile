# Stage 1: Rust builder
FROM rust:1.83 AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/
COPY migrations/ migrations/

# Build the server binary in release mode
# The worker crate is excluded from the workspace, so this is fine.
RUN cargo build --release --bin opensession-server

# Stage 2: Node builder (web frontend)
FROM node:22 AS web-builder

WORKDIR /build
COPY web/ web/
WORKDIR /build/web
RUN npm install
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy server binary
COPY --from=builder /build/target/release/opensession-server /usr/local/bin/opensession-server

# Copy web build output
COPY --from=web-builder /build/web/build /usr/share/opensession/web

# Create data directory
RUN mkdir -p /data

ENV OPENSESSION_DATA_DIR=/data
ENV OPENSESSION_WEB_DIR=/usr/share/opensession/web
ENV OPENSESSION_BASE_URL=http://localhost:3000

EXPOSE 3000

CMD ["opensession-server"]
