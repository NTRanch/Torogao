#!/bin/bash
# Run the splat projection bench on a connected Android device
# and pull the results file back. Usage:
#   ./run-on-phone.sh [PLY_PATH] [LIMIT]
# PLY_PATH defaults to the estate bonsai witness on primo;
# LIMIT empty = the full 1,157,141-splat witness (20000 = smoke).
set -euo pipefail
cd "$(dirname "$0")"

PLY="${1:-$HOME/.local/share/pax/render3d-splat-fixtures-20261125/bonsai-7000/point_cloud.ply}"
LIMIT="${2:-}"
BIN=./splat_mobile_bench
[ -x "$BIN" ] || { echo "no ./splat_mobile_bench — run ./build-phone-bench.sh first"; exit 1; }

adb push "$BIN" /data/local/tmp/splat_mobile_bench
adb push "$PLY" /data/local/tmp/bonsai.ply
adb shell chmod +x /data/local/tmp/splat_mobile_bench

LIMIT_ARG=""
[ -n "$LIMIT" ] && LIMIT_ARG="--limit $LIMIT"
adb shell "/data/local/tmp/splat_mobile_bench --ply /data/local/tmp/bonsai.ply \
    --out /data/local/tmp/bench-phone.txt --reps 3 --dispatch-reps 5 $LIMIT_ARG"
adb pull /data/local/tmp/bench-phone.txt ./bench-phone.txt
echo "results: crates/torogao-render3d/mobile-bench/bench-phone.txt"
