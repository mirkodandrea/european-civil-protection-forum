#!/bin/sh
# Build the Rust browser app; Rocket serves the generated assets directly.
set -eu
cd "$(dirname "$0")/.."
NO_COLOR=true trunk build --skip-version-check --release --config crates/frontend/Trunk.toml "$@"
for page in master player play advanced evaluator; do
    cp web/dist/index.html "web/dist/$page.html"
done
