#!/usr/bin/env bash
# Serve the workflow test bench ("Banco di prova") from the standalone WASM
# build with Trunk live reload. Simulated server and AI: no key, no backend.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
port="${PORT:-8080}"
echo "Banco di prova: http://127.0.0.1:${port}/?bench=1  (one workflow: &b=inject|suggest|excon-suggest|clarify|conversation|decide)"
NO_COLOR=true trunk serve --skip-version-check --config crates/frontend/Trunk.toml --features standalone,bench --port "$port" "$@"
