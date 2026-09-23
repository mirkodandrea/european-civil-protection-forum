#!/usr/bin/env bash
# Record a scripted TTX session: every role played in the phone-sized
# play.html (Playwright video) + the 3D simulator (timestamped frames), then
# compose both side by side into one 1080p MP4.
#
#   ./scripts/record/record.sh [output.mp4]      (default docs/media/ttx_demo.mp4)
#
# Needs: a release build (cargo build --release -p app), .env with
# OPENROUTER_API_KEY, Google Chrome, node, ffmpeg. Runs its own ttx instance on
# port 8090 in a temporary directory, so a game running on :8080 is untouched.
# A second simulator window opens while recording (keep it on screen).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT_MP4="${1:-$ROOT/docs/media/ttx_demo.mp4}"
PORT="${TTX_REC_PORT:-8090}"
WORK="$(mktemp -d -t ttx-rec)"
echo "work dir: $WORK"
mkdir -p "$WORK/run/runs/rec"
cp "$ROOT/.env" "$WORK/run/.env"

( cd "$ROOT/scripts/record" && [ -d node_modules ] || npm install --silent )

( cd "$WORK/run" && TTX_ORBIT=1 exec "$ROOT/target/release/ttx" --port "$PORT" \
    --web "$ROOT/web" --scenario "$ROOT/data/scenarios/synthetic_valley.json" > ttx.log 2>&1 ) &
TTX_PID=$!
trap 'kill $TTX_PID 2>/dev/null || true' EXIT
for _ in $(seq 60); do curl -sf "http://localhost:$PORT/api/state" >/dev/null && break; sleep 0.5; done
sleep 3 # let the window come up

( cd "$ROOT/scripts/record" && node session.js "http://localhost:$PORT" "$WORK/out" "$WORK/run/runs/rec/ON" )
mkdir -p "$(dirname "$OUT_MP4")"
python3 "$ROOT/scripts/record/compose.py" "$WORK/out" "$WORK/run/runs/rec" "$OUT_MP4" --crf "${TTX_REC_CRF:-33}"
cp "$WORK/out/marks.json" "${OUT_MP4%.mp4}_captions.json"
echo "done: $OUT_MP4"
