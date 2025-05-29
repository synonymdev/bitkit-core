#!/bin/bash

set -e  # Exit immediately if a command exits with a non-zero status.

echo "Starting iOS build process..."

# Remove previous builds and ensure clean state
echo "Cleaning previous builds..."
rm -rf bindings/ios/*
rm -rf ios/

# Create necessary directories
echo "Creating build directories..."
mkdir -p bindings/ios/

# Set iOS deployment target
export IPHONEOS_DEPLOYMENT_TARGET=13.4

# Build default target first - this is important for uniffi-bindgen
echo "Building default target..."
cargo build --release

# Modify Cargo.toml
echo "Updating Cargo.toml..."
sed -i '' 's/crate_type = .*/crate_type = ["cdylib", "staticlib"]/' Cargo.toml

# Add iOS targets
echo "Adding iOS targets..."
rustup target add aarch64-apple-ios-sim aarch64-apple-ios

# Build for iOS simulator and device
echo "Building for iOS targets..."
cargo build --release --target=aarch64-apple-ios-sim
cargo build --release --target=aarch64-apple-ios

# Generate Swift bindings
echo "Generating Swift bindings..."
# First, ensure any existing generated files are removed
rm -rf ./bindings/ios/bitkitcore.swift
rm -rf ./bindings/ios/bitkitcoreFFI.h
rm -rf ./bindings/ios/bitkitcoreFFI.modulemap
rm -rf ./bindings/ios/Headers
rm -rf ./bindings/ios/ios-arm64
rm -rf ./bindings/ios/ios-arm64-sim

cargo run --bin uniffi-bindgen generate \
    --library ./target/release/libbitkitcore.dylib \
    --language swift \
    --out-dir ./bindings/ios \
    || { echo "Failed to generate Swift bindings"; exit 1; }

# Handle modulemap file
echo "Handling modulemap file..."
if [ -f bindings/ios/bitkitcoreFFI.modulemap ]; then
    mv bindings/ios/bitkitcoreFFI.modulemap bindings/ios/module.modulemap
else
    echo "Warning: modulemap file not found"
fi

# Clean up any existing XCFramework and temporary directories
echo "Cleaning up existing XCFramework..."
rm -rf "bindings/ios/BitkitCore.xcframework"
rm -rf "bindings/ios/Headers"
rm -rf "bindings/ios/ios-arm64"
rm -rf "bindings/ios/ios-arm64-sim"

# Create temporary directories for each architecture
echo "Creating architecture-specific directories..."
mkdir -p "bindings/ios/ios-arm64/Headers"
mkdir -p "bindings/ios/ios-arm64-sim/Headers"

# Copy headers to architecture-specific directories
echo "Copying headers to architecture directories..."
cp bindings/ios/bitkitcoreFFI.h "bindings/ios/ios-arm64/Headers/"
cp bindings/ios/module.modulemap "bindings/ios/ios-arm64/Headers/"
cp bindings/ios/bitkitcoreFFI.h "bindings/ios/ios-arm64-sim/Headers/"
cp bindings/ios/module.modulemap "bindings/ios/ios-arm64-sim/Headers/"

# Create XCFramework
echo "Creating XCFramework..."
xcodebuild -create-xcframework \
    -library ./target/aarch64-apple-ios-sim/release/libbitkitcore.a -headers "bindings/ios/ios-arm64-sim/Headers" \
    -library ./target/aarch64-apple-ios/release/libbitkitcore.a -headers "bindings/ios/ios-arm64/Headers" \
    -output "bindings/ios/BitkitCore.xcframework" \
    || { echo "Failed to create XCFramework"; exit 1; }

# Clean up temporary directories
echo "Cleaning up temporary directories..."
rm -rf "bindings/ios/ios-arm64"
rm -rf "bindings/ios/ios-arm64-sim"

# Create zip file for distribution and checksum calculation
echo "Creating XCFramework zip file..."
rm -f ./bindings/ios/BitkitCore.xcframework.zip
ditto -c -k --sequesterRsrc --keepParent ./bindings/ios/BitkitCore.xcframework ./bindings/ios/BitkitCore.xcframework.zip || { echo "Failed to create zip file"; exit 1; }

# Compute checksum
echo "Computing checksum..."
CHECKSUM=`swift package compute-checksum ./bindings/ios/BitkitCore.xcframework.zip` || { echo "Failed to compute checksum"; exit 1; }
echo "New checksum: $CHECKSUM"

echo "iOS build process completed successfully!"
echo "Update Package.swift with the new checksum: $CHECKSUM"