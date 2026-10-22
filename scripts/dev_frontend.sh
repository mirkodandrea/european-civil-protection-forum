#!/usr/bin/env bash
# Serve the full standalone WASM app with Trunk's rebuild and browser reload.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Match run_pages_local.sh: read the OpenRouter key from .env and make it
# available at compile time for the standalone browser runtime.
if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    export OPENROUTER_API_KEY="$(python3 - <<'PY'
from pathlib import Path

for raw in Path('.env').read_text().splitlines() if Path('.env').is_file() else []:
    line = raw.strip()
    if not line or line.startswith('#'):
        continue
    if line.startswith('export '):
        line = line[7:].lstrip()
    name, sep, value = line.partition('=')
    if sep and name.strip() == 'OPENROUTER_API_KEY':
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
            value = value[1:-1]
        print(value, end='')
        break
PY
)"
fi

if [[ -n "$OPENROUTER_API_KEY" ]]; then
    echo "OpenRouter configurato dalla build locale; la chiave sarà inclusa nel WASM. Non pubblicare questa build."
else
    echo "Nessuna OPENROUTER_API_KEY trovata; AI in modalità manuale."
fi

echo "Development UI: http://127.0.0.1:8080 (standalone full WASM, Trunk live reload)"
NO_COLOR=true trunk serve --skip-version-check --config crates/frontend/Trunk.toml --features standalone "$@"
