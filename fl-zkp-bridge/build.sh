#!/bin/bash

# Build and install the FL-ZKP Bridge Python module

echo "Building FL-ZKP Bridge..."
echo "=========================="

# Check if maturin is installed
if ! command -v maturin &> /dev/null
then
    echo "Maturin not found. Installing..."
    pip install maturin
fi

# Build in release mode
echo "Building Rust extension module..."
cd "$(dirname "$0")"
maturin develop --release

if [ $? -eq 0 ]; then
    echo ""
    echo "✓ Build successful!"
    echo ""
    echo "You can now run the demo:"
    echo "  python examples/fl_demo.py"
    echo ""
    echo "Or use it in your Python code:"
    echo "  import fl_zkp_bridge"
    echo "  prover = fl_zkp_bridge.FLZKPProver()"
else
    echo ""
    echo "✗ Build failed!"
    exit 1
fi
