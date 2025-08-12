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

# Define output directories
ANDROID_LIB_DIR="./bindings/android"
BASE_DIR="$ANDROID_LIB_DIR/lib/src/main/kotlin/com/synonym/bitkitcore"
JNILIBS_DIR="$ANDROID_LIB_DIR/lib/src/main/jniLibs"

# Create output directories
mkdir -p "$BASE_DIR"
mkdir -p "$JNILIBS_DIR"

# Remove previous build
echo "Removing previous build..."
rm -rf "$BASE_DIR"/*
rm -rf "$JNILIBS_DIR"/*

# Cargo Build
echo "Building Rust libraries..."
cargo build

# Modify Cargo.toml
echo "Updating Cargo.toml..."
sed -i '' 's/crate_type = .*/crate_type = ["cdylib"]/' Cargo.toml

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
    --manifest-path ./Cargo.toml \
    -t armeabi-v7a \
    -t arm64-v8a \
    -t x86 \
    -t x86_64 \
    build --release

# Generate Kotlin bindings
echo "Generating Kotlin bindings..."
LIBRARY_PATH="./target/release/libbitkitcore.dylib"

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
cargo run --bin uniffi-bindgen generate \
    --library "$LIBRARY_PATH" \
    --language kotlin \
    --config uniffi.toml \
    --out-dir "$TMP_DIR"

# Move the Kotlin file from the nested directory to the final location
echo "Moving Kotlin file to final location..."
find "$TMP_DIR" -name "bitkitcore.kt" -exec mv {} "$BASE_DIR/" \;

# Clean up temp directory and any remaining uniffi directories
echo "Cleaning up temporary files..."
rm -rf "$TMP_DIR"
rm -rf "$ANDROID_LIB_DIR/uniffi"

# Verify the file was moved correctly
if [ ! -f "$BASE_DIR/bitkitcore.kt" ]; then
    echo "Error: Kotlin bindings were not moved correctly"
    echo "Contents of $BASE_DIR:"
    ls -la "$BASE_DIR"
    exit 1
fi

# Sync version
echo "Syncing version from Cargo.toml..."
CARGO_VERSION=$(grep '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/' | head -1)
sed -i.bak "s/^version=.*/version=$CARGO_VERSION/" "$ANDROID_LIB_DIR/gradle.properties"
rm -f "$ANDROID_LIB_DIR/gradle.properties.bak"

# Verify android library publish
echo "Testing android library publish to Maven Local..."
"$ANDROID_LIB_DIR"/gradlew --project-dir "$ANDROID_LIB_DIR" clean publishToMavenLocal

echo "Android build process completed successfully!"