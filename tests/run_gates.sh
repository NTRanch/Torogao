#!/usr/bin/env bash
# run_gates.sh — run the Torogao phase-gate harness suite against
# one torogao-cli binary.
#
# Usage:
#   tests/run_gates.sh <torogao-cli-binary> [rig-check-binary] [output-root]
#
#   <torogao-cli-binary>  The binary under test (required). Every
#                         harness receives it as its first argument.
#   [rig-check-binary]    The external model validator the P3/P4
#                         harnesses invoke (see tests/RIG-CHECK.md).
#                         When omitted, /bin/false stands in: the
#                         validator checks then fail honestly
#                         instead of the harnesses crashing on a
#                         missing file.
#   [output-root]         Directory for per-harness work dirs and
#                         logs. Default: a fresh mktemp directory.
#
# The harnesses are vendored byte-identical from the D development
# phases and are never edited; this runner adapts only paths and
# argument order. Their main() unpacking is the argv contract:
#   P3:  cli, fixtures, work, p2-fixtures, rig-check
#   P4:  cli, p4-fixtures, work, p3-fixtures, p2-fixtures,
#        rig-check, repo-root
#   P5:  cli, p3-fixtures, work
#   MCP: binary, fixtures, work
#   A2A: binary, fixtures, work, port
#
# The P4 repo-root argument is this repository's root: the harness
# reads its contracts, packs, scenes, and fixture rigs from there
# (data carried over from the D tree, byte-identical).
#
# Exit code: the number of failing harnesses (0 = all pass),
# capped at 125. Each harness's full output is kept in
# <output-root>/<name>.log and echoed to stdout.
set -euo pipefail

readonly A2A_PORT=8411

if [[ $# -lt 1 || $# -gt 3 ]]; then
    echo "usage: $0 <torogao-cli-binary> [rig-check-binary] [output-root]" >&2
    exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cli_binary="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
rig_check="${2:-/bin/false}"
if [[ $# -ge 2 ]]; then
    rig_check="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
fi
output_root="${3:-$(mktemp -d "${TMPDIR:-/tmp}/torogao-gates.XXXXXX")}"
mkdir -p "$output_root"

fixtures="$repo_root/tests/fixtures"
harness="$repo_root/tests/harness"

echo "run_gates: cli=$cli_binary"
echo "run_gates: rig-check=$rig_check"
echo "run_gates: output=$output_root"

failures=0
failed_names=""

# run_one <name> <command...> — run one harness into a fresh work
# dir, tee its output to its log, and record a one-line verdict.
# A failing harness is data for the summary, never a runner abort.
run_one() {
    local name="$1"
    shift
    local work="$output_root/$name"
    mkdir -p "$work"
    echo "=== GATE $name ==="
    local rc=0
    "$@" 2>&1 | tee "$output_root/$name.log" || rc="${PIPESTATUS[0]}"
    if [[ "$rc" -eq 0 ]]; then
        echo "GATE $name: PASS"
    else
        echo "GATE $name: FAIL (exit $rc)"
        failures=$((failures + 1))
        failed_names="$failed_names $name"
    fi
}

# The runner's working directory is the repo root: the P5 harness
# locates the scenes tree relative to the working directory when
# the binary-relative path does not resolve.
cd "$repo_root"

run_one p3 python3 "$harness/p3/p3_harness.py" "$cli_binary" \
    "$fixtures/p3" "$output_root/p3" "$fixtures/p2/fixtures" "$rig_check"
run_one p4 python3 "$harness/p4/p4_harness.py" "$cli_binary" \
    "$fixtures/p4" "$output_root/p4" "$fixtures/p3" \
    "$fixtures/p2/fixtures" "$rig_check" "$repo_root"
run_one p5 python3 "$harness/p5/p5_harness.py" "$cli_binary" \
    "$fixtures/p3" "$output_root/p5"
run_one mcp python3 "$harness/p15/mcp_harness.py" "$cli_binary" \
    "$fixtures/p15" "$output_root/mcp"
run_one a2a python3 "$harness/p15/a2a_harness.py" "$cli_binary" \
    "$fixtures/p15" "$output_root/a2a" "$A2A_PORT"

if [[ "$failures" -eq 0 ]]; then
    echo "GATES SUMMARY: 5/5 passed"
else
    echo "GATES SUMMARY: $((5 - failures))/5 passed; failing:$failed_names"
fi
if [[ "$failures" -gt 125 ]]; then
    failures=125
fi
exit "$failures"
