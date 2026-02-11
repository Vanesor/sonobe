#!/bin/bash

# Test script to validate the FL-ZKP Bridge setup

set -e  # Exit on error

echo "========================================================================"
echo "FL-ZKP Bridge - Setup Validation"
echo "========================================================================"
echo ""

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check Rust
echo -n "Checking Rust installation... "
if command -v rustc &> /dev/null; then
    RUST_VERSION=$(rustc --version)
    echo -e "${GREEN}✓${NC} Found: $RUST_VERSION"
else
    echo -e "${RED}✗${NC} Rust not found!"
    echo "  Install from: https://rustup.rs/"
    exit 1
fi

# Check Cargo
echo -n "Checking Cargo... "
if command -v cargo &> /dev/null; then
    CARGO_VERSION=$(cargo --version)
    echo -e "${GREEN}✓${NC} Found: $CARGO_VERSION"
else
    echo -e "${RED}✗${NC} Cargo not found!"
    exit 1
fi

# Check Python
echo -n "Checking Python installation... "
if command -v python3 &> /dev/null; then
    PYTHON_VERSION=$(python3 --version)
    echo -e "${GREEN}✓${NC} Found: $PYTHON_VERSION"
else
    echo -e "${RED}✗${NC} Python3 not found!"
    echo "  Install Python 3.8 or higher"
    exit 1
fi

# Check pip
echo -n "Checking pip... "
if command -v pip3 &> /dev/null; then
    echo -e "${GREEN}✓${NC} Found"
else
    echo -e "${YELLOW}⚠${NC} pip3 not found, trying pip..."
    if command -v pip &> /dev/null; then
        echo -e "${GREEN}✓${NC} Found pip"
    else
        echo -e "${RED}✗${NC} pip not found!"
        exit 1
    fi
fi

# Check if we're in the right directory
echo ""
echo -n "Checking directory structure... "
if [ -f "Cargo.toml" ] && [ -f "src/lib.rs" ]; then
    echo -e "${GREEN}✓${NC} In fl-zkp-bridge directory"
else
    echo -e "${RED}✗${NC} Not in fl-zkp-bridge directory!"
    echo "  Please run from: sonobe/fl-zkp-bridge/"
    exit 1
fi

# Check maturin
echo ""
echo -n "Checking maturin installation... "
if command -v maturin &> /dev/null; then
    MATURIN_VERSION=$(maturin --version)
    echo -e "${GREEN}✓${NC} Found: $MATURIN_VERSION"
else
    echo -e "${YELLOW}⚠${NC} Maturin not found"
    echo "  Installing maturin..."
    pip3 install maturin || pip install maturin
    
    if command -v maturin &> /dev/null; then
        echo -e "${GREEN}✓${NC} Maturin installed successfully"
    else
        echo -e "${RED}✗${NC} Failed to install maturin"
        exit 1
    fi
fi

# Test Rust compilation
echo ""
echo "Testing Rust compilation..."
echo -n "  Building Rust example... "
if cargo build --example addition_circuit --release &> /tmp/fl_zkp_build.log; then
    echo -e "${GREEN}✓${NC} Build successful"
else
    echo -e "${RED}✗${NC} Build failed"
    echo "  Check log: /tmp/fl_zkp_build.log"
    exit 1
fi

# Build Python module
echo ""
echo "Building Python module..."
echo -n "  Running maturin develop... "
if maturin develop --release &> /tmp/fl_zkp_maturin.log; then
    echo -e "${GREEN}✓${NC} Build successful"
else
    echo -e "${RED}✗${NC} Build failed"
    echo "  Check log: /tmp/fl_zkp_maturin.log"
    exit 1
fi

# Test Python import
echo ""
echo -n "Testing Python import... "
if python3 -c "import fl_zkp_bridge; print('Success')" &> /dev/null; then
    echo -e "${GREEN}✓${NC} Module imports correctly"
else
    echo -e "${RED}✗${NC} Import failed"
    echo "  Try: maturin develop --release"
    exit 1
fi

# Summary
echo ""
echo "========================================================================"
echo -e "${GREEN}All checks passed!${NC}"
echo "========================================================================"
echo ""
echo "You can now:"
echo "  1. Run Rust example:    cargo run --release --example addition_circuit"
echo "  2. Run Python demo:     python3 examples/fl_demo.py"
echo "  3. Run advanced demo:   python3 examples/fl_advanced.py"
echo ""
echo "Documentation:"
echo "  • Quick start:  QUICKSTART.md"
echo "  • Full docs:    README.md"
echo "  • Project info: ../PROJECT_OVERVIEW.md"
echo ""
echo "========================================================================"
