#!/bin/bash
# Build splat_mobile_bench for a physical Android device
# (aarch64-linux-android) with the estate toolchain.
# Output: ./splat_mobile_bench in mobile-bench/.
set -euo pipefail
cd "$(dirname "$0")/../../.."   # workspace root (crates/torogao-render3d/mobile-bench -> torogao/)

export PATH="$HOME/.cargo/bin:/opt/cuda/bin:$PATH"
export CUDA_HOME=/opt/cuda CUDARC_CUDA_VERSION=13040
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
NDK=/opt/android-sdk/ndk/30.0.16248370
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang

cargo +1.100.0-x86_64-unknown-linux-gnu build --release \
    --target aarch64-linux-android -p torogao-render3d --bin splat_mobile_bench

cp "$CARGO_TARGET_DIR/aarch64-linux-android/release/splat_mobile_bench" \
   "crates/torogao-render3d/mobile-bench/splat_mobile_bench"
echo "built: crates/torogao-render3d/mobile-bench/splat_mobile_bench"
