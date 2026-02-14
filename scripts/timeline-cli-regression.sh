#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

echo "[timeline-cli] running CLI timeline linear/timing regression tests..."
cargo test -p opensession --test timeline_cli_linear -- --nocapture

echo "[timeline-cli] done"
