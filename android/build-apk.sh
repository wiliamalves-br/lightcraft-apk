#!/usr/bin/env bash
# Build the LightCraft Android APK (arm64): the Rust app as liblightcraft_android.so, then the
# Gradle project around it. Output: target/lightcraft-android.apk
#
# Needs: Rust (rustup), the Android SDK (ANDROID_HOME) with an NDK (ANDROID_NDK_HOME, or
# ANDROID_NDK_LATEST_HOME on GitHub runners), JDK 17 and Gradle 8.9+ (or GRADLE=/path/to/gradle).
set -euo pipefail
cd "$(dirname "$0")/.."

export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_ROOT:-}}}"
if [ -z "$ANDROID_NDK_HOME" ]; then
    echo "error: set ANDROID_NDK_HOME to your Android NDK" >&2
    exit 1
fi

rustup target add aarch64-linux-android
command -v cargo-ndk >/dev/null || cargo install cargo-ndk --locked

# release, without debug info (the library is large enough), symbols stripped
export CARGO_PROFILE_RELEASE_DEBUG=false
export CARGO_PROFILE_RELEASE_STRIP=symbols
cargo ndk -t arm64-v8a --platform 26 -o android/app/src/main/jniLibs build --release -p lightcraft-android

# launcher icons from the app icon (not kept in android/: generated, like the library)
for pair in mdpi:48x48 hdpi:64x64 xhdpi:128x128 xxhdpi:256x256 xxxhdpi:512x512; do
    density="${pair%%:*}"
    size="${pair##*:}"
    mkdir -p "android/app/src/main/res/mipmap-$density"
    cp -f "assets/app-icon/hicolor/$size/apps/ai.storyteller.lightcraft.png" "android/app/src/main/res/mipmap-$density/ic_launcher.png"
done

(cd android && "${GRADLE:-gradle}" --no-daemon assembleRelease)
mkdir -p target
cp -f android/app/build/outputs/apk/release/app-release.apk target/lightcraft-android.apk
echo "APK: target/lightcraft-android.apk"
