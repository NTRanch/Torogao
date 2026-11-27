#!/usr/bin/env bash
# Theodolite cured-protocol gate for bundle-native rung 2 (D-1124).
# Rust: producer (bacon) first with a planted E0308 control in the
# tree (session A), control removed + producer re-run (session B).
# C#: same-session planted error appended to LayoutBuilder.cs,
# restored by md5-verified copy.
set -u
CRATE=$HOME/.local/share/pax/bundle-native-20261110
W=$HOME/.local/share/pax/bundle-native-rung2-20261110
PROBE=$W/rung2-tools/probe_case.py
EVID=$W/evidence
mkdir -p "$EVID"

wait_locations() { # $1 = string that must be present ("" = must be absent-ish/empty file exists), $2 = seconds
  local want="$1" secs="$2" waited=0
  while [ $waited -lt "$secs" ]; do
    if [ -f "$CRATE/.bacon-locations" ]; then
      if [ -z "$want" ]; then
        if ! grep -q control_scratch "$CRATE/.bacon-locations"; then return 0; fi
      else
        if grep -q "$want" "$CRATE/.bacon-locations"; then return 0; fi
      fi
    fi
    sleep 5; waited=$((waited + 5))
  done
  return 1
}

start_bacon() {
  (cd "$CRATE" && nohup bacon --headless -j bacon-ls > "$EVID/bacon-producer.log" 2>&1 & echo $! > "$EVID/bacon.pid")
  sleep 2
}

stop_bacon() {
  if [ -f "$EVID/bacon.pid" ]; then kill "$(cat "$EVID/bacon.pid")" 2>/dev/null; sleep 2; fi
}

cp "$CRATE/src/lib.rs" "$EVID/lib.rs.bak-theodolite"

echo "=== SESSION A (planted E0308 control present) ==="
cat > "$CRATE/src/control_scratch.rs" <<'EOF'
// Planted positive control for the theodolite gate (D-1124):
// a deliberate type error that MUST surface as diagnostics.
pub fn control() -> u32 {
    let x: u32 = "not a number";
    x
}
EOF
grep -q "mod control_scratch" "$CRATE/src/lib.rs" || printf '\nmod control_scratch;\n' >> "$CRATE/src/lib.rs"
rm -f "$CRATE/.bacon-locations"
start_bacon
if wait_locations control_scratch 240; then echo "producer: control error present in .bacon-locations"; else echo "producer: TIMEOUT waiting for control error"; fi
OUT=$EVID/theodolite-rung2-session-a.txt; : > "$OUT"
for f in src/control_scratch.rs src/layout.rs src/monoscript.rs src/synth.rs src/join.rs src/value.rs; do
  echo "=== $f" >> "$OUT"
  CASE_TIMEOUT=600 timeout 700 python3 "$PROBE" bacon-ls "$CRATE" "$f" >> "$OUT" 2>&1
  echo "=== $f rc=$?" >> "$OUT"
done
stop_bacon

echo "=== SESSION B (control removed) ==="
rm -f "$CRATE/src/control_scratch.rs"
cp "$EVID/lib.rs.bak-theodolite" "$CRATE/src/lib.rs"
rm -f "$CRATE/.bacon-locations"
start_bacon
if wait_locations "" 240; then echo "producer: clean locations regenerated"; else echo "producer: TIMEOUT waiting for clean locations"; fi
OUT=$EVID/theodolite-rung2-session-b.txt; : > "$OUT"
for f in src/layout.rs src/monoscript.rs src/synth.rs src/join.rs src/value.rs; do
  echo "=== $f" >> "$OUT"
  CASE_TIMEOUT=600 timeout 700 python3 "$PROBE" bacon-ls "$CRATE" "$f" >> "$OUT" 2>&1
  echo "=== $f rc=$?" >> "$OUT"
done
stop_bacon
md5sum "$CRATE/src/lib.rs"
echo "RUST SESSIONS DONE"
