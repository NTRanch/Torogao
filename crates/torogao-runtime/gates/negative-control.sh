#!/usr/bin/env bash
# negative-control.sh — the twin gate's negative control
# (runtime plan SS2.4: the gate is only proven when a
# deliberate fault is caught by the right tier and not the
# other).
#
# Applies gates/negative-control.patch — a +1.25e-3
# perturbation of the interpreter witness's day_progress
# gauge, a Tier 2 float in its present() path — rebuilds the
# native harness, and requires the exact verdict pattern:
#
#   Tier 1 comparison vs the unperturbed reference: PASS
#     (the perturbation lives in Tier 2; the sim chain, the
#     checkpoint save hash, and the replay verdict must not
#     move — if they do, a float has leaked into Tier 1);
#   Tier 2 band comparison vs the reference: FAIL
#     (the measured distance must exceed the recorded band;
#     if it passes, the band instrument has no teeth).
#
# The patch is then reverted and the harness rebuilt, so the
# tree is left exactly as found. Run twin-gate.sh first (or
# pass its output dir) so a reference emit exists.
#
# Usage: negative-control.sh [twin-gate-output-dir]
# Exit: 0 = the control behaved exactly as required.

set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RUNTIME_CRATE=$(cd "$SCRIPT_DIR/.." && pwd)
WS_ROOT=$(cd "$RUNTIME_CRATE/../.." && pwd)
CORPUS="$RUNTIME_CRATE/corpus/interpreter.corpus.json"
OUT=${1:-$(mktemp -d /tmp/twin-gate-neg.XXXXXX)}
mkdir -p "$OUT"
REFERENCE="$OUT/interpreter.native.1.txt"
PERTURBED="$OUT/interpreter.native.perturbed.txt"

if [[ ! -f $REFERENCE ]]; then
    "$WS_ROOT/target/release/runtime-twin" emit "$CORPUS" > "$REFERENCE"
fi

cleanup() {
    (cd "$WS_ROOT" && git apply -R "$SCRIPT_DIR/negative-control.patch" 2>/dev/null || true)
    (cd "$WS_ROOT" && cargo build --release -p torogao-runtime --bin runtime-twin 2>/dev/null)
}
trap cleanup EXIT

(cd "$WS_ROOT" && git apply "$SCRIPT_DIR/negative-control.patch")
(cd "$WS_ROOT" && cargo build --release -p torogao-runtime --bin runtime-twin 2>/dev/null)
"$WS_ROOT/target/release/runtime-twin" emit "$CORPUS" > "$PERTURBED"

echo "negative-control: Tier 1 must PASS against the perturbed build"
python3 "$SCRIPT_DIR/compare_twin.py" tier1 "$REFERENCE" "$PERTURBED"

echo "negative-control: Tier 2 band must FAIL against the perturbed build"
if python3 "$SCRIPT_DIR/compare_twin.py" band 1e-6 1e-6 "$REFERENCE" "$PERTURBED"; then
    echo "negative-control: FAILED — the band did not catch a 1.25e-3 perturbation"
    exit 1
fi

echo "negative-control: PASS (Tier 1 unmoved, band caught the perturbation)"
