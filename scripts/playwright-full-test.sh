#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

HEADED=0
KEEP_UP=0
REBUILD=0
TEST_PORT="${OPS_PLAYWRIGHT_PORT:-3310}"

usage() {
  cat <<'EOF'
Usage: scripts/playwright-full-test.sh [--headed] [--keep-up] [--rebuild]

Runs full Playwright E2E tests against the Dockerized opensession server.

Options:
  --headed   Run Playwright in headed mode
  --keep-up  Keep Docker services running after tests
  --rebuild  Force docker compose image rebuild before test

Environment:
  OPS_PLAYWRIGHT_PORT  Host port for dockerized server (default: 3310)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --headed)
      HEADED=1
      shift
      ;;
    --keep-up)
      KEEP_UP=1
      shift
      ;;
    --rebuild)
      REBUILD=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

COMPOSE_ARGS=(-f docker-compose.yml -f docker-compose.test.yml)
SERVICE="opensession"
BASE_URL="http://localhost:${TEST_PORT}"

cleanup() {
  if [[ "$KEEP_UP" -eq 0 ]]; then
    docker compose "${COMPOSE_ARGS[@]}" down --remove-orphans >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ "$REBUILD" -eq 1 ]]; then
  OPENSESSION_HTTP_PORT="$TEST_PORT" docker compose "${COMPOSE_ARGS[@]}" up -d --build
else
  OPENSESSION_HTTP_PORT="$TEST_PORT" docker compose "${COMPOSE_ARGS[@]}" up -d
fi

container_id="$(OPENSESSION_HTTP_PORT="$TEST_PORT" docker compose "${COMPOSE_ARGS[@]}" ps -q "$SERVICE")"
if [[ -z "$container_id" ]]; then
  echo "Failed to resolve docker container for service: $SERVICE" >&2
  exit 1
fi

echo "Waiting for $SERVICE health check..."
for _ in $(seq 1 60); do
  status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$container_id")"
  if [[ "$status" == "healthy" || "$status" == "running" ]]; then
    break
  fi
  sleep 2
done

status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$container_id")"
if [[ "$status" != "healthy" && "$status" != "running" ]]; then
  echo "Container did not become healthy (status: $status)" >&2
  docker compose "${COMPOSE_ARGS[@]}" logs "$SERVICE" || true
  exit 1
fi

if [[ ! -d web/node_modules ]]; then
  pnpm --dir web install --frozen-lockfile
fi

pnpm --dir web exec playwright install chromium

if [[ "$HEADED" -eq 1 ]]; then
  BASE_URL="$BASE_URL" pnpm --dir web exec playwright test --headed
else
  BASE_URL="$BASE_URL" pnpm --dir web exec playwright test
fi

echo "Playwright full-test completed."
