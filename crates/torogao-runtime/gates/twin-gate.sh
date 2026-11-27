#!/usr/bin/env bash
# twin-gate.sh — the cross-architecture twin gate (runtime
# plan SS2.4/SS4.1, landed Rung R2).
#
# Runs both R1 witness corpora, from their committed file
# form, on three executed targets:
#
#   native   Linux x86_64, run natively (the reference run)
#   gnu      Linux aarch64 (aarch64-unknown-linux-gnu),
#            run under qemu-aarch64 user-mode
#   android  Android aarch64 (aarch64-linux-android),
#            run under qemu-aarch64 user-mode
#
# and judges, per witness:
#   1. within-target twin runs are byte-identical (Tier 1
#      and Tier 2 alike — one architecture must reproduce
#      itself exactly, the D-1397 twin criterion);
#   2. Tier 1 identity lines are byte-identical across all
#      three targets (no tolerance, ever);
#   3. Tier 2 gauge values sit inside the recorded band
#      across targets (compare_twin.py prints the measured
#      max distances — those numbers are the gate record);
#   4. the cross-target save gate: every target's checkpoint
#      save is byte-identical, and every producer/consumer
#      target pair continues it to the identical chain;
#   5. the estate-rng/1 golden vectors pass natively and
#      under qemu on the aarch64 build.
#
# The physical ARM device column is a NAMED OPEN GATE
# (R0 record SS3): no device was available; this script is
# the harness that runs on it unchanged when one joins.
#
# Provisioning baked in here (do not rediscover per run):
#   - the aarch64-linux-gnu linker pin lives in the
#     workspace .cargo/config.toml (R0, structural);
#   - qemu needs the sysroot flag -L /usr/aarch64-linux-gnu
#     for dynamically linked aarch64-linux-gnu binaries;
#   - the Android leg runs with TMPDIR pointed at the gate
#     scratch dir: Rust temp_dir on Android defaults to
#     /data/local/tmp, which a qemu-user process on a Linux
#     host does not have (on a device, the app sandbox is
#     what provides a writable temp dir);
#   - the Android-target harness is linked fully static
#     (crt-static) so qemu-user can run it without a bionic
#     loader: that needs --allow-multiple-definition (the
#     NDK's libc.a and Rust's std both define
#     rust_eh_personality) and the NDK's clang builtins
#     archive for the outline-atomics helpers. These flags
#     are scoped to this gate's harness builds only — the
#     shipped Android route (dynamic .so in an APK) does not
#     use them.
#
# Usage: twin-gate.sh [output-dir]   (default: mktemp dir)
# Exit: 0 = every check green; non-zero = first failure.

set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RUNTIME_CRATE=$(cd "$SCRIPT_DIR/.." && pwd)
WS_ROOT=$(cd "$RUNTIME_CRATE/../.." && pwd)
LOOP_TREE=${LOOP_TREE:-"$HOME/.local/share/pax/ntr-rebuild-loop-service-20261116/build"}
CORPUS_DIR="$RUNTIME_CRATE/corpus"
OUT=${1:-$(mktemp -d /tmp/twin-gate.XXXXXX)}
mkdir -p "$OUT" "$OUT/tmp"

export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-/opt/android-sdk/ndk/30.0.16248370}
QEMU=${QEMU:-qemu-aarch64}
AARCH64_SYSROOT=${AARCH64_SYSROOT:-/usr/aarch64-linux-gnu}
BAND_ABS=1e-6
BAND_REL=1e-6
TARGETS="native gnu android"

ANDROID_BUILTINS=$(echo "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/linux-x86_64/lib/clang/*/lib/linux/libclang_rt.builtins-aarch64-android.a)
ANDROID_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-Wl,--allow-multiple-definition -C link-arg=$ANDROID_BUILTINS"

echo "twin-gate: output in $OUT"
echo "twin-gate: building harnesses (torogao + loop trees, 3 targets each)"

build_tree() {
    # $1 = tree root, $2 = package, $3 = bin name
    local root=$1 package=$2 bin=$3
    (cd "$root" && cargo build --release -p "$package" --bin "$bin" 2>/dev/null)
    (cd "$root" && cargo build --release --target aarch64-unknown-linux-gnu -p "$package" --bin "$bin" 2>/dev/null)
    (cd "$root" && RUSTFLAGS="$ANDROID_RUSTFLAGS" cargo ndk -t arm64-v8a build --release -p "$package" --bin "$bin" 2>/dev/null)
}

build_tree "$WS_ROOT" torogao-runtime runtime-twin
build_tree "$WS_ROOT" torogao-runtime runtime-clip-twin
build_tree "$LOOP_TREE" torogao-loop-host loop-twin

bin_for() {
    # $1 = tree root, $2 = target, $3 = bin name
    case $2 in
        native)  echo "$1/target/release/$3" ;;
        gnu)     echo "$1/target/aarch64-unknown-linux-gnu/release/$3" ;;
        android) echo "$1/target/aarch64-linux-android/release/$3" ;;
    esac
}

run_target() {
    # $1 = target, $2 = binary, rest = args
    local target=$1 binary=$2
    shift 2
    case $target in
        native)  "$binary" "$@" ;;
        gnu)     "$QEMU" -L "$AARCH64_SYSROOT" "$binary" "$@" ;;
        android) TMPDIR="$OUT/tmp" "$QEMU" "$binary" "$@" ;;
    esac
}

witness_bin() {
    # $1 = witness, $2 = target
    case $1 in
        interpreter) bin_for "$WS_ROOT" "$2" runtime-twin ;;
        clip)        bin_for "$WS_ROOT" "$2" runtime-clip-twin ;;
        loop)        bin_for "$LOOP_TREE" "$2" loop-twin ;;
    esac
}

for witness in interpreter loop clip; do
    corpus="$CORPUS_DIR/$witness.corpus.json"
    echo "twin-gate: witness $witness"
    for target in $TARGETS; do
        bin=$(witness_bin "$witness" "$target")
        run_target "$target" "$bin" emit "$corpus" > "$OUT/$witness.$target.1.txt"
        run_target "$target" "$bin" emit "$corpus" > "$OUT/$witness.$target.2.txt"
        cmp "$OUT/$witness.$target.1.txt" "$OUT/$witness.$target.2.txt"
        echo "  twin-run byte-identical: $target"
    done
    python3 "$SCRIPT_DIR/compare_twin.py" tier1 \
        "$OUT/$witness.native.1.txt" "$OUT/$witness.gnu.1.txt" "$OUT/$witness.android.1.txt"
    python3 "$SCRIPT_DIR/compare_twin.py" band "$BAND_ABS" "$BAND_REL" \
        "$OUT/$witness.native.1.txt" "$OUT/$witness.gnu.1.txt" "$OUT/$witness.android.1.txt"
    # Cross-target save gate: 3 producers x 3 consumers.
    for producer in $TARGETS; do
        run_target "$producer" "$(witness_bin "$witness" "$producer")" \
            save "$corpus" "$OUT/$witness.save.$producer" > /dev/null
    done
    cmp "$OUT/$witness.save.native" "$OUT/$witness.save.gnu"
    cmp "$OUT/$witness.save.native" "$OUT/$witness.save.android"
    echo "  save envelopes byte-identical across targets"
    for producer in $TARGETS; do
        for consumer in $TARGETS; do
            run_target "$consumer" "$(witness_bin "$witness" "$consumer")" \
                continue "$corpus" "$OUT/$witness.save.$producer" \
                > "$OUT/$witness.cont.$producer.$consumer.txt"
        done
    done
    for producer in $TARGETS; do
        for consumer in $TARGETS; do
            cmp "$OUT/$witness.cont.native.native.txt" "$OUT/$witness.cont.$producer.$consumer.txt"
        done
    done
    echo "  save continuations identical for all 9 producer/consumer pairs"
done

echo "twin-gate: estate-rng/1 golden vectors"
(cd "$WS_ROOT" && cargo test -p torogao-author rng_golden 2>/dev/null | grep -E "test result" | head -1)
(cd "$WS_ROOT" && cargo test -p torogao-author --target aarch64-unknown-linux-gnu --no-run 2>/dev/null)
GOLDEN_BIN=$(find "$WS_ROOT/target/aarch64-unknown-linux-gnu/debug/build/torogao-author" -path "*/out/torogao_author-*" -type f -executable | head -1)
"$QEMU" -L "$AARCH64_SYSROOT" "$GOLDEN_BIN" rng_golden 2>/dev/null | grep -E "test result"

echo "twin-gate: ALL CHECKS GREEN"
echo "twin-gate: physical-device column: OPEN (no physical aarch64 device available; qemu legs above are the executed ARM evidence)"
