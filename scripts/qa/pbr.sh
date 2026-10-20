#!/bin/sh
# Native, invisible GPU regression captures. No HTTP server or LLM is started.
set -eu
cd "$(dirname "$0")/../.."
out=${1:-runs/pbr-review/verified}
if [ "$#" -gt 0 ]; then shift; fi
mkdir -p "$out"
cargo build --offline --release -p app
for view in unit units town fire overhead-ui low-rollback; do
  quality=${TTX_QUALITY:-projector}
  ui=0
  marks=0
  case "$view" in
    unit) camera='2450,2000,230,0.5,0.38'; fixture=work;;
    units) camera='1640,1370,430,0.4,0.4'; fixture=pbr-units;;
    town) camera='1660,1270,650,0.65,0.55'; fixture=quiet;;
    fire) camera='2800,2700,1500,0.7,0.5'; fixture=fire;;
    overhead-ui) camera='2600,2250,3200,0,1.45'; fixture=fire; ui=1; marks=1;;
    low-rollback) camera='2450,2000,230,0.5,0.38'; fixture=rollback; quality=low;;
  esac
  if ! TTX_SNAPSHOT="$out/$view.png" TTX_QUALITY="$quality" \
    TTX_CAM="$camera" TTX_SNAPSHOT_UI="$ui" TTX_SNAPSHOT_MARKS="$marks" \
    TTX_SNAPSHOT_SPEC="scripts/visual-fixtures/$fixture.json" \
    target/release/ttx --operational-model --seed 1 --realizations 4 "$@" > "$out/$view.log" 2>&1; then
    tail -60 "$out/$view.log"
    exit 1
  fi
  # A PNG can still be saved after a shader pipeline failure. Treat it as a
  # failed capture, even when the application returned zero.
  if rg -n 'ERROR|panicked|failed to process shader|Validation Error' "$out/$view.log"; then
    exit 1
  fi
  test -s "$out/$view.png"
  printf '%s\n' "GPU capture OK: $out/$view.png ($quality)"
done
# Human/agent visual inspection of every PNG is still required.
