#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage:
  collect-desktop-diagnostics.sh \
    --out-dir <path> \
    --os <label> \
    --bundle-dir <path> \
    --app-bin <path> \
    [--build-seconds <int>] \
    [--smoke-log <path>]
EOF
}

OUT_DIR=""
OS_LABEL=""
BUNDLE_DIR=""
APP_BIN=""
BUILD_SECONDS="0"
SMOKE_LOG=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --os) OS_LABEL="$2"; shift 2 ;;
    --bundle-dir) BUNDLE_DIR="$2"; shift 2 ;;
    --app-bin) APP_BIN="$2"; shift 2 ;;
    --build-seconds) BUILD_SECONDS="$2"; shift 2 ;;
    --smoke-log) SMOKE_LOG="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1"; usage; exit 1 ;;
  esac
done

if [ -z "$OUT_DIR" ] || [ -z "$OS_LABEL" ] || [ -z "$BUNDLE_DIR" ] || [ -z "$APP_BIN" ]; then
  usage
  exit 1
fi

mkdir -p "$OUT_DIR"

{
  echo "timestamp_utc=$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  echo "os=$OS_LABEL"
  echo "bundle_dir=$BUNDLE_DIR"
  echo "app_bin=$APP_BIN"
  echo "build_seconds=$BUILD_SECONDS"
} > "$OUT_DIR/context.txt"

if [ -d "$BUNDLE_DIR" ]; then
  find "$BUNDLE_DIR" -maxdepth 4 -print | sort > "$OUT_DIR/bundle-tree.txt"
else
  echo "missing bundle dir: $BUNDLE_DIR" > "$OUT_DIR/bundle-tree.txt"
fi

if [ -f "$APP_BIN" ]; then
  file "$APP_BIN" > "$OUT_DIR/file-output.txt" 2>&1 || true
  if command -v lipo >/dev/null 2>&1; then
    lipo -archs "$APP_BIN" > "$OUT_DIR/lipo-output.txt" 2>&1 || true
  fi
  APP_BYTES=$(wc -c < "$APP_BIN" | tr -d ' ')
else
  echo "missing app binary: $APP_BIN" > "$OUT_DIR/file-output.txt"
  APP_BYTES=0
fi

DMG_PATH=""
if [ -d "$BUNDLE_DIR/dmg" ]; then
  DMG_PATH=$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -type f -name '*.dmg' | head -n 1 || true)
fi

if [ -n "$DMG_PATH" ] && [ -f "$DMG_PATH" ]; then
  DMG_BYTES=$(wc -c < "$DMG_PATH" | tr -d ' ')
else
  DMG_BYTES=0
fi

if [ -n "$SMOKE_LOG" ] && [ -f "$SMOKE_LOG" ]; then
  cp "$SMOKE_LOG" "$OUT_DIR/smoke.log"
fi

cat > "$OUT_DIR/metrics.json" <<EOF
{
  "timestamp_utc": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "os": "$OS_LABEL",
  "build_seconds": $BUILD_SECONDS,
  "app_bytes": $APP_BYTES,
  "dmg_bytes": $DMG_BYTES
}
EOF
