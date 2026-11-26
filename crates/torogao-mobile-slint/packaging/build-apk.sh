#!/usr/bin/env bash
# build-apk.sh — package Torogao Mobile (Slint shell) into a signed,
# sideloadable fat APK (arm64-v8a + x86_64), Gradle-free.
#
# Adapted from crates/torogao-coin-android/packaging/build-apk.sh
# (the estate's proven hand-rolled pipeline: aapt2 link -> zip the
# native libs in stored -> zipalign -> apksigner). Differences from
# the coin spike: two ABIs, a classes.dex (Slint's Java helper), a
# dedicated release keystore (never the debug key), and NDK 30 —
# Slint's prebuilt Skia requires NDK r27+ libc++ symbols and its
# android-activity build script needs the SDK platform jar and d8
# pinned explicitly on this host.
#
# Contract:
#   in : crates/torogao-mobile-slint (this crate), the Android SDK
#        at /opt/android-sdk, cargo-ndk on PATH.
#   out: packaging/torogao-mobile.apk (+ .sha256), signed with the
#        dedicated key at packaging/torogao-mobile.keystore
#        (generated on first run; password in
#        packaging/.keystore-pass, both git-ignored — the key IS
#        the app's update identity, keep it).
# Every step fails the script on error; nothing is half-packaged.
set -euo pipefail

export ANDROID_NDK_HOME=/opt/android-sdk/ndk/30.0.16248370
export ANDROID_HOME=/opt/android-sdk
export ANDROID_SDK_ROOT=/opt/android-sdk
# Pinned for i-slint-backend-android-activity's build script
# (android-build crate): on this host its SDK discovery otherwise
# finds no platform.
export ANDROID_JAR=/opt/android-sdk/platforms/android-36/android.jar
export ANDROID_PLATFORM=android-36
export ANDROID_D8_JAR=/opt/android-sdk/build-tools/37.0.0/lib/d8.jar

BT=/opt/android-sdk/build-tools/37.0.0
ANDROID_JAR_HOST=/opt/android-sdk/platforms/android-36/android.jar
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE="$(dirname "$HERE")"
WT="$(cd "$CRATE/../.." && pwd)"
OUT="$HERE/torogao-mobile.apk"
WORK="$HERE/.apk-work"
KS="$HERE/torogao-mobile.keystore"
PASS_FILE="$HERE/.keystore-pass"

for tool in cargo-ndk aapt2 zipalign apksigner keytool; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
[ -x "$BT/aapt2" ] && [ -x "$BT/zipalign" ] && [ -x "$BT/apksigner" ] \
    || { echo "build-tools 37.0.0 incomplete under $BT" >&2; exit 1; }

echo "== cargo ndk: release libs (arm64-v8a, x86_64) =="
cd "$WT"
cargo ndk -t arm64-v8a -t x86_64 build --release --lib -p torogao-mobile-slint

ARM_SO="$WT/target/aarch64-linux-android/release/libtorogao_mobile_slint.so"
X64_SO="$WT/target/x86_64-linux-android/release/libtorogao_mobile_slint.so"
[ -f "$ARM_SO" ] && [ -f "$X64_SO" ] || { echo "native libs missing" >&2; exit 1; }

DEXES=$(find "$WT/target/aarch64-linux-android/release/build" \
    -path '*i-slint-backend-android-activity-*/out/classes.dex')
[ "$(printf '%s' "$DEXES" | grep -c .)" = "1" ] \
    || { echo "expected exactly one classes.dex, found: $DEXES" >&2; exit 1; }

echo "== keystore =="
if [ ! -f "$KS" ]; then
    umask 077
    openssl rand -base64 24 > "$PASS_FILE"
    keytool -genkeypair -keystore "$KS" -alias torogao-mobile \
        -keyalg RSA -keysize 4096 -validity 10957 \
        -storepass "$(cat "$PASS_FILE")" -keypass "$(cat "$PASS_FILE")" \
        -dname "CN=Torogao Mobile, O=Nice Thoughts Ranch" >/dev/null 2>&1
    echo "generated dedicated release keystore: $KS"
fi
[ -f "$PASS_FILE" ] || { echo "keystore exists but $PASS_FILE is missing" >&2; exit 1; }
STOREPASS="$(cat "$PASS_FILE")"

echo "== aapt2 link =="
rm -rf "$WORK"
mkdir -p "$WORK/stage/lib/arm64-v8a" "$WORK/stage/lib/x86_64"
cp "$ARM_SO" "$WORK/stage/lib/arm64-v8a/"
cp "$X64_SO" "$WORK/stage/lib/x86_64/"
cp "$DEXES" "$WORK/stage/classes.dex"
"$BT/aapt2" link \
    -I "$ANDROID_JAR_HOST" \
    --manifest "$HERE/AndroidManifest.xml" \
    --min-sdk-version 26 --target-sdk-version 36 \
    -o "$WORK/base.apk"

echo "== zip native libs + dex (stored, page-aligned) =="
( cd "$WORK/stage" && zip -q -0 -r "$WORK/base.apk" lib classes.dex )

echo "== zipalign + sign =="
"$BT/zipalign" -f -p 4 "$WORK/base.apk" "$WORK/aligned.apk"
"$BT/apksigner" sign \
    --ks "$KS" --ks-pass "pass:$STOREPASS" --key-pass "pass:$STOREPASS" \
    --out "$OUT" "$WORK/aligned.apk"
"$BT/apksigner" verify --print-certs "$OUT" | head -4
sha256sum "$OUT" | tee "$OUT.sha256"
ls -la "$OUT"
