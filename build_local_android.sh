#!/usr/bin/env bash
# Build the local development artifact consumed by the Pubky integration branch.
set -euo pipefail
cd "$(dirname "$0")"

local_version="${LOCAL_BITKIT_CORE_VERSION:-0.5.14-pubky-swap-boltz-local}"
# The Bitkit app already bundles this helper through Paykit.
bundle_tls_helper="${LOCAL_BUNDLE_PUBKY_TLS_HELPER:-false}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$ANDROID_NDK_HOME" || ! -d "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" ]]; then
    echo "Set ANDROID_NDK_HOME to an installed Android NDK." >&2
    exit 1
fi
if [[ -z "${JAVA_HOME:-}" ]]; then
    echo "Set JAVA_HOME to JDK 17 or 21." >&2
    exit 1
fi
command -v cargo-ndk >/dev/null
command -v gobley-uniffi-bindgen >/dev/null

# Native symbols are archived before stripping. Existing Gradle checks validate
# every packaged ABI. Build only the shared library consumed by Android.
export CARGO_PROFILE_RELEASE_DEBUG=2
export CARGO_PROFILE_RELEASE_STRIP=false
cargo ndk -o bindings/android/lib/src/main/jniLibs \
    -t armeabi-v7a -t arm64-v8a -t x86 -t x86_64 rustc --lib --crate-type cdylib --release --locked

# Binding metadata can be read directly from the Android library.
binding_library=target/aarch64-linux-android/release/libbitkitcore.so

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
python3 bindings/android/prepare_tls_helper.py
gobley-uniffi-bindgen --library "$binding_library" --config uniffi-android.toml --out-dir "$scratch/bindings"
binding_dir=bindings/android/lib/src/main/kotlin/com/synonym/bitkitcore
mkdir -p "$binding_dir"
find "$scratch/bindings" -name '*.kt' -exec cp {} "$binding_dir/" \;

# APFS cloning avoids slow sparse-file copies of the large debug libraries.
copy_native_library() {
    if [[ "$(uname -s)" == Darwin ]] && cp -c "$1" "$2"; then
        return
    fi
    cp "$1" "$2"
}

strip_tool=$(find "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt" -path '*/bin/llvm-strip' -print -quit)
[[ -x "$strip_tool" ]]
for abi in armeabi-v7a arm64-v8a x86 x86_64; do
    mkdir -p "$scratch/symbols/$abi"
    case "$abi" in
        armeabi-v7a) rust_target=armv7-linux-androideabi ;;
        arm64-v8a) rust_target=aarch64-linux-android ;;
        x86) rust_target=i686-linux-android ;;
        x86_64) rust_target=x86_64-linux-android ;;
    esac
    lib="bindings/android/lib/src/main/jniLibs/$abi/libbitkitcore.so"
    mkdir -p "$(dirname "$lib")"
    copy_native_library "target/$rust_target/release/libbitkitcore.so" "$lib"
    copy_native_library "$lib" "$scratch/symbols/$abi/libbitkitcore.so"
    "$strip_tool" --strip-unneeded "$lib"
done
archive="$PWD/bindings/android/native-debug-symbols.zip"
rm -f "$archive"
(cd "$scratch/symbols" && zip -qr "$archive" .)
(cd bindings/android && ./gradlew :lib:publishMavenPublicationToMavenLocal -Pversion="$local_version" -PbundlePubkyTlsHelper="$bundle_tls_helper")
