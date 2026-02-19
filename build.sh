#!/bin/sh
set -e

run_cargo_stable() {
    if command -v rustup >/dev/null 2>&1; then
        RUSTC_BIN=$(rustup which --toolchain stable rustc 2>/dev/null || true)
        CARGO_BIN=$(rustup which --toolchain stable cargo 2>/dev/null || true)
        if [ -n "$RUSTC_BIN" ] && [ -n "$CARGO_BIN" ]; then
            TOOLCHAIN_BIN=$(dirname "$RUSTC_BIN")
            PATH="$TOOLCHAIN_BIN:$PATH" RUSTC="$RUSTC_BIN" "$CARGO_BIN" "$@"
            return
        fi
    fi

    cargo "$@"
}

run_with_stable_rustc() {
    if command -v rustup >/dev/null 2>&1; then
        RUSTC_BIN=$(rustup which --toolchain stable rustc 2>/dev/null || true)
        if [ -n "$RUSTC_BIN" ]; then
            TOOLCHAIN_BIN=$(dirname "$RUSTC_BIN")
            PATH="$TOOLCHAIN_BIN:$PATH" RUSTC="$RUSTC_BIN" "$@"
            return
        fi
    fi

    "$@"
}

# Frontend
cd packages/ui && npm install && cd ../..
cd web && npm install && npm run build && cd ..

# Worker (skip in CI where it's built separately with cache)
if [ -z "$SKIP_WORKER_BUILD" ]; then
    if command -v rustup >/dev/null 2>&1; then
        rustup target add wasm32-unknown-unknown --toolchain stable >/dev/null
    fi

    if ! command -v worker-build >/dev/null 2>&1; then
        run_cargo_stable install -q worker-build --locked
    fi

    cd crates/worker
    run_with_stable_rustc worker-build --release
    cd ../..

    # worker-build outputs to crates/worker/build; wrangler expects build/ at root
    rm -rf build
    cp -R crates/worker/build ./build
fi
