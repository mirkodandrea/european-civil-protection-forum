#!/bin/sh
# Main's original UI and model, fully local. No credentials are embedded.
set -eu
cd "$(dirname "$0")/.."
cargo build --locked --release -p standalone --target wasm32-unknown-unknown
mkdir -p web/standalone
wasm-bindgen --target web --out-dir web/standalone --out-name standalone "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/standalone.wasm"
python3 scripts/package_standalone.py
