#!/usr/bin/env bash
# Run from any directory; preserve every result bundle in a fresh destination.
set -euo pipefail
repo="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repo"
suite="${1:-scripts/lab/valley.json}"
out="${2:-/tmp/ttx-lab-$(date +%Y%m%d-%H%M%S)}"
cargo run --release --offline -p sim --bin operational_lab -- "$suite" "$out"
python3 scripts/lab/report.py "$out"
