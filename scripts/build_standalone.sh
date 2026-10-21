#!/bin/sh
# Build the whole exercise into WASM, then embed every asset in one HTML file.
set -eu
cd "$(dirname "$0")/.."
NO_COLOR=true trunk build --skip-version-check --locked --release --config crates/frontend/Trunk.toml --features standalone --dist ../../web/standalone
python3 scripts/package_standalone.py web/standalone
