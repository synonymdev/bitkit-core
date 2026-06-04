#!/bin/bash

#TODO: Eventually remove cleanup when example/main.rs builds successfully
# Define cleanup function
cleanup() {
    local exit_code=$?
    echo "Performing cleanup..."

    # Restore example/main.rs
    if [ ! -z "$MAIN_RS_CONTENT" ]; then
        echo "Restoring example/main.rs..."
        mkdir -p "example"
        echo "$MAIN_RS_CONTENT" > "example/main.rs"
        echo "example/main.rs restored"
    fi

    # Restore bin configuration in Cargo.toml
    if [ ! -z "$BIN_CONFIG" ]; then
        echo "Restoring bin configuration in Cargo.toml..."
        echo "$BIN_CONFIG" >> Cargo.toml
        echo "Bin configuration restored"
    fi

    if [ $exit_code -ne 0 ]; then
        echo "Script failed with exit code $exit_code"
    fi
    exit $exit_code
}

# Set up trap to call cleanup function on script exit
trap cleanup EXIT

set -e  # Exit immediately if a command exits with a non-zero status.

echo "Starting Android build process..."

# Install gobley-uniffi-bindgen from fork with patched version
echo "Installing gobley-uniffi-bindgen fork..."
cargo install --git https://github.com/ovitrif/gobley.git gobley-uniffi-bindgen --force

#TODO: Remove this section when example/main.rs builds successfully
# Store example/main.rs content in memory and remove the file
if [ -f "example/main.rs" ]; then
    echo "Backing up example/main.rs..."
    MAIN_RS_CONTENT=$(cat "example/main.rs")
    rm "example/main.rs"
    echo "example/main.rs temporarily removed"
fi

#TODO: Remove this section when example/main.rs builds successfully
# Backup and remove bin configuration from Cargo.toml
echo "Backing up and removing bin configuration from Cargo.toml..."
if grep -q '\[\[bin\]\]' Cargo.toml; then
    # Store the bin configuration lines
    BIN_CONFIG=$(awk '/\[\[bin\]\]/,/^$/' Cargo.toml)
    # Remove the bin configuration section
    sed -i.bak '/\[\[bin\]\]/,/^$/d' Cargo.toml
    rm -f Cargo.toml.bak
    echo "Bin configuration temporarily removed"
fi

# Set OpenSSL environment variables
export OPENSSL_STATIC=1
export OPENSSL_NO_VENDOR=0
export CARGO_PROFILE_RELEASE_DEBUG=2
export CARGO_PROFILE_RELEASE_STRIP=false

# Define output directories
ANDROID_LIB_DIR="./bindings/android"
BASE_DIR="$ANDROID_LIB_DIR/lib/src/main/kotlin/com/synonym/bitkitcore"
JNILIBS_DIR="$ANDROID_LIB_DIR/lib/src/main/jniLibs"

# Create output directories
mkdir -p "$BASE_DIR"
mkdir -p "$JNILIBS_DIR"

find_readelf() {
    if command -v llvm-readelf >/dev/null 2>&1; then
        command -v llvm-readelf
        return
    fi

    if command -v readelf >/dev/null 2>&1; then
        command -v readelf
        return
    fi

    echo "Error: llvm-readelf or readelf is required to validate Android native debug symbols"
    exit 1
}

has_debug_metadata() {
    "$READELF_BIN" -S "$1" | grep -Eq '\.(symtab|debug_|gnu_debugdata)'
}

validate_android_symbols() {
    READELF_BIN=$(find_readelf)

    for abi in armeabi-v7a arm64-v8a x86 x86_64; do
        lib="$JNILIBS_DIR/$abi/libbitkitcore.so"
        if [ ! -f "$lib" ]; then
            echo "Error: Android native library missing at $lib"
            exit 1
        fi

        if ! has_debug_metadata "$lib"; then
            echo "Error: Android native library has no usable debug metadata: $lib"
            exit 1
        fi
    done
}

host_library_path() {
    case "$(uname -s)" in
        Darwin)
            echo "./target/release/libbitkitcore.dylib"
            ;;
        Linux)
            echo "./target/release/libbitkitcore.so"
            ;;
        *)
            echo "Error: Unsupported host OS for Kotlin binding generation: $(uname -s)" >&2
            exit 1
            ;;
    esac
}

# Remove previous build
echo "Removing previous build..."
rm -rf "$BASE_DIR"/*
rm -rf "$JNILIBS_DIR"/*

# Cargo Build
echo "Building Rust libraries..."
cargo build

# Build release
echo "Building release version..."
cargo build --release

# Install cargo-ndk if not already installed
if ! command -v cargo-ndk &> /dev/null; then
    echo "Installing cargo-ndk..."
    cargo install cargo-ndk
fi

# Add Android targets
echo "Adding Android targets..."
rustup target add \
    aarch64-linux-android \
    armv7-linux-androideabi \
    i686-linux-android \
    x86_64-linux-android

# Build for all Android architectures
echo "Building for Android architectures..."
cargo ndk \
    -o "$JNILIBS_DIR" \
    --no-strip \
    --manifest-path ./Cargo.toml \
    -t armeabi-v7a \
    -t arm64-v8a \
    -t x86 \
    -t x86_64 \
    build --release

validate_android_symbols

# Generate Kotlin bindings
echo "Generating Kotlin bindings..."
LIBRARY_PATH=$(host_library_path)

# Check if the library file exists
if [ ! -f "$LIBRARY_PATH" ]; then
    echo "Error: Library file not found at $LIBRARY_PATH"
    echo "Available files in target/release:"
    ls -l ./target/release/
    exit 1
fi

# Create a temporary directory for initial generation
TMP_DIR=$(mktemp -d)

# Generate the bindings to temp directory first
gobley-uniffi-bindgen --library "$LIBRARY_PATH" \
    --config uniffi-android.toml \
    --out-dir "$TMP_DIR"

# Move the Kotlin files from the nested directory to the final location
echo "Moving Kotlin files to final location..."
mv "$TMP_DIR"/main/kotlin/com/synonym/bitkitcore/*.kt "$BASE_DIR/"

# Clean up temp directory and any remaining uniffi directories
echo "Cleaning up temporary files..."
rm -rf "$TMP_DIR"
rm -rf "$ANDROID_LIB_DIR/uniffi"

# Verify the files were moved correctly
if [ ! -f "$BASE_DIR/bitkitcore.android.kt" ] || [ ! -f "$BASE_DIR/bitkitcore.common.kt" ]; then
    echo "Error: Kotlin bindings were not moved correctly"
    echo "Contents of $BASE_DIR:"
    ls -la "$BASE_DIR"
    exit 1
fi

echo "Generated Kotlin bindings:"
ls -la "$BASE_DIR"

# Sync version
echo "Syncing version from Cargo.toml..."
CARGO_VERSION=$(grep '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/' | head -1)
sed -i.bak "s/^version=.*/version=$CARGO_VERSION/" "$ANDROID_LIB_DIR/gradle.properties"
rm -f "$ANDROID_LIB_DIR/gradle.properties.bak"

# Verify android library publish
echo "Testing android library publish to Maven Local..."
"$ANDROID_LIB_DIR"/gradlew --project-dir "$ANDROID_LIB_DIR" clean publishToMavenLocal

echo "Android build process completed successfully!"
