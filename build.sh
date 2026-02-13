#!/bin/sh
set -e

# Frontend
cd packages/ui && npm install && cd ../..
cd web && npm install && npm run build && cd ..

# Worker (skip in CI where it's built separately with cache)
if [ -z "$SKIP_WORKER_BUILD" ]; then
    cargo install -q worker-build && cd crates/worker && worker-build --release && cd ../..
fi
