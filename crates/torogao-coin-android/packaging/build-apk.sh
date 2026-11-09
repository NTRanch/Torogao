#!/usr/bin/env bash
# Torogao Coin spike — Gradle-free APK packaging (debug-signed).
#
# Adaptation of the Hands-On/MoonDeep pipeline (build-apk.sh of
# record) to this estate's current SDK: every version is RESOLVED
# from what is installed and echoed, never assumed — the spike
# report records the resolved SDK platform, build-tools, and NDK
# (ANDROID-SPIKE-BRIEF.md Scope 5 / STOP-2). The NDK is pinned to
# the 28 line by the brief; if no NDK 28 resolves, this script
# stops and reports instead of silently taking another.
#
# Pipeline: cargo-ndk builds libtorogao_coin_android.so -> aapt2
# links the manifest (+ a generated launcher icon) against
# android.jar -> the .so is added under lib/arm64-v8a/ stored
# (uncompressed, page-aligned via zipalign -p) -> apksigner signs
# with a DEBUG keystore. Debug signing is for the spike's
# sideload proof only.
#
# Run from the torogao workspace root, through a login shell
# (bash -lc) so the tree-pinned toolchain resolves.
set -euo pipefail

SDK="${ANDROID_SDK_ROOT:-/opt/android-sdk}"
PKG="crates/torogao-coin-android/packaging"
OUT="$PKG/out"
KEYSTORE="$PKG/debug.keystore"

BT="$(ls -d "$SDK"/build-tools/*/ | sort -V | tail -1)"
JAR="$(ls -d "$SDK"/platforms/*/ | sort -V | tail -1)android.jar"
echo "build-tools: $BT"
echo "platform jar: $JAR"

# NDK 28 resolution (brief Scope 5): the SDK's 28 install wins;
# the standalone /opt/android-ndk is taken only if its revision
# is a 28 line. The resolved path is echoed for the report.
NDK=""
for cand in "$SDK"/ndk/28.* /opt/android-ndk; do
    if [ -f "$cand/source.properties" ] && grep -q '^Pkg.Revision = 28\.' "$cand/source.properties"; then
        NDK="$cand"
        break
    fi
done
if [ -z "$NDK" ]; then
    echo "STOP: no NDK 28 resolved under $SDK/ndk or /opt/android-ndk" >&2
    exit 1
fi
export ANDROID_NDK_HOME="$NDK"
echo "ndk: $NDK ($(grep '^Pkg.Revision' "$NDK/source.properties"))"

echo "== cargo-ndk build (arm64-v8a, release) =="
cargo ndk -t arm64-v8a build --release -p torogao-coin-android

mkdir -p "$OUT/lib/arm64-v8a" "$OUT/res/mipmap-hdpi"
cp target/aarch64-linux-android/release/libtorogao_coin_android.so "$OUT/lib/arm64-v8a/"

# A C++ runtime ships only if the NDK sysroot offers it AND a
# future dependency needs it; the spike's graph is pure Rust,
# so this stays conditional exactly as in the pipeline of
# record.
CXXLIB="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so"
if [ -f "$CXXLIB" ] && grep -q 'libc++_shared' "$OUT/lib/arm64-v8a/libtorogao_coin_android.so"; then
    cp "$CXXLIB" "$OUT/lib/arm64-v8a/"
    echo "bundled libc++_shared.so (linked by the shell)"
fi

echo "== launcher icon (generated placeholder art) =="
python3 - "$OUT/res/mipmap-hdpi/ic_launcher.png" <<'PY'
import struct, sys, zlib

w = h = 72
rows = []
for y in range(h):
    row = bytearray([0])
    for x in range(w):
        # amber disc on near-black — the studio accent
        dx, dy = x - w / 2, y - h / 2
        inside = dx * dx + dy * dy < (w * 0.36) ** 2
        row += bytes((224, 169, 46, 255) if inside else (26, 20, 16, 255))
    rows.append(bytes(row))
raw = b"".join(rows)


def chunk(tag, data):
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


png = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(raw))
    + chunk(b"IEND", b"")
)
open(sys.argv[1], "wb").write(png)
PY

echo "== aapt2 compile + link =="
"$BT/aapt2" compile --dir "$OUT/res" -o "$OUT/res.zip"
"$BT/aapt2" link \
    -I "$JAR" \
    --manifest "$PKG/AndroidManifest.xml" \
    "$OUT/res.zip" \
    --auto-add-overlay \
    -o "$OUT/unsigned.apk"

echo "== add native libs (stored) + align =="
(cd "$OUT" && zip -q -0 unsigned.apk lib/arm64-v8a/libtorogao_coin_android.so)
if [ -f "$OUT/lib/arm64-v8a/libc++_shared.so" ]; then
    (cd "$OUT" && zip -q -0 unsigned.apk lib/arm64-v8a/libc++_shared.so)
fi
"$BT/zipalign" -f -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"

echo "== debug keystore + sign =="
if [ ! -f "$KEYSTORE" ]; then
    "${JAVA_HOME:?set JAVA_HOME (keytool)}/bin/keytool" -genkeypair -v \
        -keystore "$KEYSTORE" -storepass android -keypass android \
        -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Android Debug,O=Android,C=US"
fi
"$BT/apksigner" sign \
    --ks "$KEYSTORE" --ks-pass pass:android --key-pass pass:android \
    --out "$OUT/torogao-coin-spike-debug.apk" "$OUT/aligned.apk"

echo "== verify =="
"$BT/apksigner" verify --print-certs "$OUT/torogao-coin-spike-debug.apk"
"$BT/aapt2" dump badging "$OUT/torogao-coin-spike-debug.apk" | head -5
ls -l "$OUT/torogao-coin-spike-debug.apk"
sha256sum "$OUT/torogao-coin-spike-debug.apk"
echo "APK-DONE"
