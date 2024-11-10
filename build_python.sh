#!/bin/bash

set -e  # Exit immediately if a command exits with a non-zero status.

echo "Starting Python build process..."

# Define output directories and package info
PROJECT_NAME="bitkitcore"  # Using underscore for Python package name
BASE_DIR="./bindings/python"
PACKAGE_DIR="$BASE_DIR/$PROJECT_NAME"

# Create output directories
mkdir -p "$BASE_DIR"
mkdir -p "$PACKAGE_DIR"

# Remove previous build
echo "Removing previous build..."
# shellcheck disable=SC2115
rm -rf "$PACKAGE_DIR"/*

# Cargo Build
echo "Building Rust libraries..."
cargo build

# Modify Cargo.toml to ensure correct crate type
echo "Updating Cargo.toml..."
sed -i '' 's/crate_type = .*/crate_type = ["cdylib"]/' Cargo.toml

# Build release
echo "Building release version..."
cargo build --release

# Generate Python bindings
echo "Generating Python bindings..."
LIBRARY_PATH="./target/release/libbitkitcore.dylib"

# Check if the library file exists
if [ ! -f "$LIBRARY_PATH" ]; then
    echo "Error: Library file not found at $LIBRARY_PATH"
    echo "Available files in target/release:"
    ls -l ./target/release/
    exit 1
fi

# Generate the Python bindings
cargo run --bin uniffi-bindgen generate \
    --library "$LIBRARY_PATH" \
    --language python \
    --out-dir "$PACKAGE_DIR"

# Create __init__.py
touch "$PACKAGE_DIR/__init__.py"

# Create setup.py
cat > "$BASE_DIR/setup.py" << EOL
from setuptools import setup, find_packages

setup(
    name="$PROJECT_NAME",
    version="0.1.0",
    packages=find_packages(),
    package_data={
        "$PROJECT_NAME": ["*.so", "*.dylib", "*.dll"],
    },
    install_requires=[],
    author="Synonym",
    author_email="",
    description="Bitcoin & Lightning invoice parsing library",
    long_description=open("README.md").read(),
    long_description_content_type="text/markdown",
    url="",
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
    ],
    python_requires=">=3.6",
)
EOL

# Create README.md with updated example
cat > "$BASE_DIR/README.md" << EOL
# Bitkit Scanner Logic

A Python library for parsing Bitcoin and Lightning Network invoices.

## Installation

\`\`\`bash
pip install .
\`\`\`

## Usage

\`\`\`python
from bitkitcore import Scanner, DecodingError

# Example Lightning invoice
lightning_invoice = "lightning:lnbc543210n1pnjdrvfpp5s720f4z6wzvjwpdnrlpffgct375l46yu9c6cpe7gdvvdfay47cnsdqqcqzzsxqrrsssp53uty4kfw8k3wmw4ga802udavz7e64tc7dmaz2cmtkj9srfxaq3ps9p4gqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpqysgqwl2tdhzm9e6mtedt7a4263yw7dqxehdwjnjk23r4g8tuppk6rs994f6scunwsev3w207tjldwkpdt32rcegzphgk05c0lctv8he7smgqyfn5xq"

try:
    result = Scanner.decode(lightning_invoice)
    if isinstance(result, Scanner.Lightning):
        print(f"Amount (sats): {result.invoice.amount_satoshis}")
        print(f"Payment Hash: {result.invoice.payment_hash.hex()}")
        if result.invoice.description:
            print(f"Description: {result.invoice.description}")
        print(f"Network: {result.invoice.network_type}")
        print(f"Is Expired: {result.invoice.is_expired}")

# Example Bitcoin invoice
    bitcoin_invoice = "bitcoin:bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq?amount=0.00001&label=Test&message=Test%20Payment"
    result = Scanner.decode(bitcoin_invoice)
    if isinstance(result, Scanner.OnChain):
        print(f"Address: {result.invoice.address}")
        print(f"Amount (sats): {result.invoice.amount_satoshis}")
        if result.invoice.label:
            print(f"Label: {result.invoice.label}")
        if result.invoice.message:
            print(f"Message: {result.invoice.message}")

except DecodingError as e:
    print(f"Failed to decode invoice: {e}")
\`\`\`

The library supports:
- Lightning Network BOLT-11 invoices
- Bitcoin addresses (with BIP-21 parameters)
- PubKey authentication strings
EOL

# Copy necessary library files
echo "Copying library files..."
case "$(uname)" in
    "Darwin")
        cp "$LIBRARY_PATH" "$PACKAGE_DIR/"
        ;;
    "Linux")
        cp "./target/release/libbitkitcore.so" "$PACKAGE_DIR/"
        ;;
    "MINGW"*|"MSYS"*|"CYGWIN"*)
        cp "./target/release/bitkitcore.dll" "$PACKAGE_DIR/"
        ;;
esac

echo "Python build process completed successfully!"
echo "To install the package, cd into $BASE_DIR and run: pip install ."