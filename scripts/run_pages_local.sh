#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

ttx_pages_port="${1:-${TTX_PAGES_PORT:-8000}}"
if [[ ! "$ttx_pages_port" =~ ^[0-9]+$ ]] || ((ttx_pages_port < 1 || ttx_pages_port > 65535)); then
    echo "Porta non valida: $ttx_pages_port" >&2
    exit 2
fi

./scripts/build_standalone.sh
mkdir -p _site
cp web/standalone/ttx.html _site/index.html
touch _site/.nojekyll

echo "Build GitHub Pages servita su http://127.0.0.1:${ttx_pages_port}/"
exec python3 -m http.server "$ttx_pages_port" --bind 127.0.0.1 --directory _site
