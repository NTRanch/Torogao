#!/usr/bin/env bash
# Torogao — Gradle-free APK packaging for the Android
# shell (Stage A-1), following the estate's Hands-On
# packaging pattern file for file.
#
# Pipeline: cargo-ndk builds libtorogao_android.so ->
# aapt2 links the manifest (+ a generated placeholder
# launcher icon) against android.jar -> the .so is added
# under lib/<abi>/ stored (uncompressed, page-aligned
# via zipalign -p) -> apksigner signs with a generated
# DEBUG keystore. Debug signing only: A-1 ships no
# release signer (A-4 owns signing provenance).
#
# Usage: build-apk.sh [arm64-v8a|x86_64]   (default arm64-v8a)
#   arm64-v8a is the target of record (aarch64-linux-android);
#   x86_64 is the identical configuration built for the
#   estate's x86_64 emulator images — the gate report
#   names which artifact ran where, never conflating them.
#
# Required env: ANDROID_SDK_ROOT (platforms/android-34,
# build-tools/34.0.0, ndk/28.0.12674087), JAVA_HOME
# (keytool), cargo-ndk on PATH. Run from the repository
# root, in a login shell (rustup on PATH).
set -euo pipefail

ABI="${1:-arm64-v8a}"
case "$ABI" in
    arm64-v8a) TARGET="aarch64-linux-android" ;;
    x86_64)    TARGET="x86_64-linux-android" ;;
    *) echo "unknown ABI: $ABI (want arm64-v8a or x86_64)" >&2; exit 2 ;;
esac

SDK="${ANDROID_SDK_ROOT:?set ANDROID_SDK_ROOT}"
BT="$SDK/build-tools/34.0.0"
JAR="$SDK/platforms/android-34/android.jar"
PKG="crates/torogao-android/packaging"
OUT="$PKG/out-$ABI"
KEYSTORE="$PKG/out/debug.keystore"

export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$SDK/ndk/28.0.12674087}"

echo "== cargo-ndk build ($ABI, release) =="
cargo ndk -t "$ABI" build --release -p torogao-android

mkdir -p "$OUT/lib/$ABI" "$OUT/res/mipmap-hdpi" "$PKG/out"
cp "target/$TARGET/release/libtorogao_android.so" "$OUT/lib/$ABI/"

echo "== launcher icon (generated placeholder art) =="
python3 - "$OUT/res/mipmap-hdpi/ic_launcher.png" <<'PY'
import struct, sys, zlib

w = h = 72
rows = []
for y in range(h):
    row = bytearray([0])
    for x in range(w):
        # violet disc on near-black — Prima's house violet
        dx, dy = x - w / 2, y - h / 2
        inside = dx * dx + dy * dy < (w * 0.36) ** 2
        row += bytes((122, 90, 248, 255) if inside else (10, 10, 14, 255))
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
(cd "$OUT" && zip -q -0 "unsigned.apk" "lib/$ABI/libtorogao_android.so")
"$BT/zipalign" -f -p 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"

echo "== debug keystore + sign =="
if [ ! -f "$KEYSTORE" ]; then
    "$JAVA_HOME/bin/keytool" -genkeypair -v \
        -keystore "$KEYSTORE" -storepass android -keypass android \
        -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Android Debug,O=Android,C=US"
fi
"$BT/apksigner" sign \
    --ks "$KEYSTORE" --ks-pass pass:android --key-pass pass:android \
    --out "$OUT/torogao-a1-$ABI-debug.apk" "$OUT/aligned.apk"

echo "== verify =="
"$BT/apksigner" verify --print-certs "$OUT/torogao-a1-$ABI-debug.apk"
"$BT/aapt2" dump badging "$OUT/torogao-a1-$ABI-debug.apk" | head -5
ls -l "$OUT/torogao-a1-$ABI-debug.apk"
sha256sum "$OUT/torogao-a1-$ABI-debug.apk"
echo "APK-DONE $ABI"
