#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

ttx_pages_port="${1:-${TTX_PAGES_PORT:-8000}}"
if [[ ! "$ttx_pages_port" =~ ^[0-9]+$ ]] || ((ttx_pages_port < 1 || ttx_pages_port > 65535)); then
    echo "Porta non valida: $ttx_pages_port" >&2
    exit 2
fi

ttx_openrouter_api_key="$(python3 - <<'PY'
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
OPENROUTER_API_KEY="$ttx_openrouter_api_key" ./scripts/build_standalone.sh
mkdir -p _site
cp web/standalone/ttx.html _site/index.html
touch _site/.nojekyll

ttx_pages_url="http://127.0.0.1:${ttx_pages_port}/"
if [[ -n "$ttx_openrouter_api_key" ]]; then
    echo "OpenRouter configurato dalla build locale. L’artefatto contiene la chiave: non pubblicarlo."
else
    echo "Nessuna chiave API in .env; AI in modalità manuale."
fi
unset ttx_openrouter_api_key

python3 -m http.server "$ttx_pages_port" --bind 127.0.0.1 --directory _site &
ttx_server_pid=$!
cleanup() {
    kill "$ttx_server_pid" 2>/dev/null || true
    wait "$ttx_server_pid" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

ttx_ready=false
for _ in {1..50}; do
    if curl --silent --fail "$ttx_pages_url" -o /dev/null; then
        ttx_ready=true
        break
    fi
    sleep 0.1
done
if [[ "$ttx_ready" != true ]]; then
    echo "Il server locale non risponde su $ttx_pages_url" >&2
    exit 1
fi

if [[ "$(uname -s)" == Darwin ]] && command -v open >/dev/null 2>&1; then
    open -a "Google Chrome" "$ttx_pages_url"
elif command -v google-chrome >/dev/null 2>&1; then
    google-chrome "$ttx_pages_url" >/dev/null 2>&1 &
elif command -v google-chrome-stable >/dev/null 2>&1; then
    google-chrome-stable "$ttx_pages_url" >/dev/null 2>&1 &
elif command -v chromium >/dev/null 2>&1; then
    chromium "$ttx_pages_url" >/dev/null 2>&1 &
else
    echo "Chrome non trovato: apri manualmente $ttx_pages_url"
fi

echo "Build GitHub Pages locale attiva su $ttx_pages_url (Ctrl+C per terminare)."
wait "$ttx_server_pid"
