#!/usr/bin/env bash
# Build the Ourcraft Android APK.
#   ./build.sh            debug-friendly build (Rust in release mode for speed)
# Needs: Android SDK + NDK, `rustup target add aarch64-linux-android`,
#        `cargo install cargo-ndk`, and a JDK (17+).
set -euo pipefail
cd "$(dirname "$0")"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)"
  export ANDROID_NDK_HOME
fi

# 1. Compile the game into libourcraft_mobile.so for 64-bit ARM.
cargo ndk -t arm64-v8a -P 31 -o app/src/main/jniLibs \
  build --release -p ourcraft_mobile --lib

# 2. Package it into an APK with Gradle.
./gradlew assembleRelease

echo
echo "APK: $(pwd)/app/build/outputs/apk/release/app-release.apk"
echo "Install on a connected device:  adb install -r app/build/outputs/apk/release/app-release.apk"
