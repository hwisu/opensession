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

stable_target_dir_name() {
    if command -v rustup >/dev/null 2>&1; then
        RUSTC_BIN=$(rustup which --toolchain stable rustc 2>/dev/null || true)
        if [ -n "$RUSTC_BIN" ]; then
            RELEASE=$("$RUSTC_BIN" -Vv 2>/dev/null | awk '/^release:/ { print $2; exit }')
            if [ -n "$RELEASE" ]; then
                printf 'target-rustup-%s\n' "$(printf '%s' "$RELEASE" | tr '.' '_')"
                return
            fi
        fi
    fi
}

prune_rust_build_artifacts() {
    rm -rf target/debug/incremental

    for path in target-rustup-*/debug/incremental; do
        [ -d "$path" ] || continue
        rm -rf "$path"
    done

    current_target_dir=$(stable_target_dir_name)
    for path in target-rustup-*; do
        [ -d "$path" ] || continue
        if [ -n "$current_target_dir" ] && [ "$(basename "$path")" = "$current_target_dir" ]; then
            continue
        fi
        rm -rf "$path"
    done
}

prune_rust_build_artifacts

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
