#!/usr/bin/env bash
# Record the narrated demo: every role played in the phone-sized play.html
# (Playwright, encoded while recording) next to the 3D simulator (streamed into
# ffmpeg while recording, cinematic camera), with Gemini TTS narration and
# subtitles. One final ffmpeg pass composes everything.
#
#   ./scripts/record/record.sh [output.mp4]      (default docs/media/ttx_demo.mp4)
#
# Environment:
#   TTX_REC_FAST=1    hardware encoder for the final pass (faster, bigger file)
#   TTX_REC_FRESH=1   ignore the LLM cassette and call the model again
#   TTX_TTS_VOICE     Gemini voice (default in narration.json: Charon)
#   TTX_REC_PORT      port of the recording ttx instance (default 8090)
#
# Speed: model answers are replayed from scripts/record/llm_cassette.jsonl and
# narration audio from scripts/record/.cache/tts, so a re-recording takes about
# the video's own length plus ~20 s. The first run (or TTX_REC_FRESH=1) calls
# the LLM and the TTS, and records both.
#
# Needs: cargo build --release -p app, .env with OPENROUTER_API_KEY, Google
# Chrome, node, ffmpeg. Runs its own ttx in a temporary directory, so a game on
# :8080 is untouched. A second simulator window opens; keep it on screen.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REC="$ROOT/scripts/record"
OUT_MP4="${1:-$ROOT/docs/media/ttx_demo.mp4}"
PORT="${TTX_REC_PORT:-8090}"
WORK="$(mktemp -d -t ttx-rec)"
T0=$(date +%s)
phase() { echo "[$(( $(date +%s) - T0 ))s] $*"; }
phase "work dir: $WORK"
mkdir -p "$WORK/run/runs/rec" "$WORK/out"
cp "$ROOT/.env" "$WORK/run/.env"
( cd "$REC" && [ -d node_modules ] || npm install --silent )

phase "narration (TTS, cached)"
node "$REC/tts.js" "$WORK/out"
node "$REC/render_subs.js" "$WORK/out"

CASSETTE_MODE=auto
[ "${TTX_REC_FRESH:-0}" = "1" ] && CASSETTE_MODE=record
phase "simulator (cinematic camera, LLM cassette: $CASSETTE_MODE)"
( cd "$WORK/run" && TTX_CINEMATIC=1 TTX_LLM_CASSETTE="$REC/llm_cassette.jsonl" TTX_LLM_CASSETTE_MODE=$CASSETTE_MODE \
    exec "$ROOT/target/release/ttx" --port "$PORT" --web "$ROOT/web" --scenario "$ROOT/data/scenarios/synthetic_valley.json" > ttx.log 2>&1 ) &
TTX_PID=$!
trap 'kill $TTX_PID 2>/dev/null || true' EXIT
for _ in $(seq 60); do curl -sf "http://localhost:$PORT/api/state" >/dev/null && break; sleep 0.5; done
sleep 3 # let the window come up

phase "session"
( cd "$REC" && node session.js "http://localhost:$PORT" "$WORK/out" "$WORK/run/runs/rec/ON" "$WORK/out/narration.json" )

phase "waiting for the simulator stream to close"
for _ in $(seq 60); do
  if ffprobe -v error "$WORK/run/runs/rec/sim.mkv" >/dev/null 2>&1 && ! pgrep -f "runs/rec/sim.mkv" >/dev/null; then break; fi
  sleep 0.5
done

phase "compose"
mkdir -p "$(dirname "$OUT_MP4")"
FAST=""; [ "${TTX_REC_FAST:-0}" = "1" ] && FAST="--fast"
python3 "$REC/compose.py" "$WORK/out" "$WORK/run/runs/rec" "$OUT_MP4" $FAST
cp "$WORK/out/marks.json" "${OUT_MP4%.mp4}_captions.json"
cp "$WORK"/run/runs/run-*.jsonl "${OUT_MP4%.mp4}_log.jsonl"
phase "done: $OUT_MP4 (+ .srt, .vtt, _captions.json, _log.jsonl)"
