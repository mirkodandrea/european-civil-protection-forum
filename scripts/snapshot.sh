#!/bin/sh
# Invisible, no-focus snapshots; each starts the same seed/scenario anew.
set -eu
cd "$(dirname "$0")/.."
out=${1:-runs/visual-review}
if [ "$#" -gt 0 ]; then shift; fi
mkdir -p "$out"
cargo build --offline --release -p app
for view in wide detail overhead; do
  case "$view" in
    wide) camera='2600,2250,2600,0.6,0.62';;
    detail) camera='2600,2250,1100,0.9,0.45';;
    overhead) camera='2600,2250,3200,0,1.45';;
  esac
  TTX_SNAPSHOT="$out/$view.png" TTX_CAM="$camera" TTX_SNAPSHOT_UI=0 TTX_SNAPSHOT_MARKS=0 \
    target/release/ttx --port 8099 --seed 1 "$@"
 done
