#!/bin/bash

bump_version() {
    local bump_type="$1"
    
    # Read current version from Cargo.toml
    local current_version=$(grep '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
    
    if [ -z "$current_version" ]; then
        echo "Error: Could not find version in Cargo.toml"
        exit 1
    fi
    
    # Parse version into parts (x.y.z)
    IFS='.' read -ra VERSION_PARTS <<< "$current_version"
    local major="${VERSION_PARTS[0]}"
    local minor="${VERSION_PARTS[1]}"
    local patch="${VERSION_PARTS[2]}"
    
    # Increment based on bump type
    case "$bump_type" in
        major)
            major=$((major + 1))
            minor=0
            patch=0
            ;;
        minor)
            minor=$((minor + 1))
            patch=0
            ;;
        patch)
            patch=$((patch + 1))
            ;;
        *)
            echo "Error: Unknown bump type: $bump_type (must be major, minor, or patch)"
            exit 1
            ;;
    esac
    
    local new_version="${major}.${minor}.${patch}"
    
    echo "Bumping $bump_type version from $current_version to $new_version"
    
    # Update Cargo.toml
    sed -i '' "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml
    
    # Update Package.swift
    sed -i '' "s/let tag = \"v.*\"/let tag = \"v$new_version\"/" Package.swift
    
    # Update gradle.properties
    sed -i '' "s/^version=.*/version=$new_version/" bindings/android/gradle.properties
    
    echo "Version bumped to $new_version in all files"
}

BUILD_TARGET=""
DO_RELEASE=false
BUMP_TYPE="patch"

while [[ $# -gt 0 ]]; do
    case $1 in
        -r|--release)
            DO_RELEASE=true
            shift
            ;;
        --major|-M)
            BUMP_TYPE="major"
            shift
            ;;
        --minor|-m)
            BUMP_TYPE="minor"
            shift
            ;;
        --patch|-p)
            BUMP_TYPE="patch"
            shift
            ;;
        ios|android|python|all)
            BUILD_TARGET="$1"
            shift
            ;;
        *)
            echo "Usage: $0 [-r|--release] [--major|-M|--minor|-m|--patch|-p] {ios|android|python|all}"
            echo "  Version bump defaults to patch if --release is specified"
            exit 1
            ;;
    esac
done

# Bump version if release flag is set
if [ "$DO_RELEASE" = true ]; then
    bump_version "$BUMP_TYPE"
fi

# Build based on target
case "$BUILD_TARGET" in
  "ios")
    ./build_ios.sh
    ;;
  "android")
    ./build_android.sh
    ;;
  "python")
    ./build_python.sh
    ;;
  "all")
    ./build_ios.sh && ./build_android.sh && ./build_python.sh
    ;;
  *)
    echo "Usage: $0 [-r|--release] [--major|-M|--minor|-m|--patch|-p] {ios|android|python|all}"
    echo "  Version bump defaults to patch if --release is specified"
    exit 1
    ;;
esac