#!/bin/sh
# Build the whole exercise into WASM, then embed every asset in one HTML file.
# The dev benches ship too (`?bench=1`, `?simbench=1`); the app ignores them otherwise.
set -eu
cd "$(dirname "$0")/.."
NO_COLOR=true trunk build --skip-version-check --locked --release --config crates/frontend/Trunk.toml --features standalone,bench --dist ../../web/standalone
python3 scripts/package_standalone.py web/standalone
