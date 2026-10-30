#!/bin/sh
# Build the Rust browser app; Rocket serves the generated assets directly.
set -eu
cd "$(dirname "$0")/.."
# TTX_BENCH=1 adds the dev-only workflow test bench (bench.html).
pages="master player play advanced evaluator observer maptest"
if [ "${TTX_BENCH:-0}" = 1 ]; then
    set -- --features bench "$@"
    pages="$pages bench"
fi
NO_COLOR=true trunk build --skip-version-check --release --config crates/frontend/Trunk.toml "$@"
cp crates/frontend/assets/favicon.ico web/dist/favicon.ico
rm -f web/dist/bench.html
for page in $pages; do
    cp web/dist/index.html "web/dist/$page.html"
done
