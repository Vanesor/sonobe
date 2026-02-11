# FL-ZKP System: Quick Start Guide

## Overview

This is a complete Zero-Knowledge Proof (ZKP) system for Federated Learning built on the Sonobe library. It implements an **addition circuit** that proves correct gradient aggregation while maintaining privacy.

## What's Inside

```
fl-zkp-bridge/
├── src/
│   └── lib.rs              # Main implementation with PyO3 bindings
├── examples/
│   ├── fl_demo.py          # Basic usage example
│   ├── fl_advanced.py      # Advanced FL integration
│   └── README.md           # Examples documentation
├── Cargo.toml              # Rust dependencies
├── pyproject.toml          # Python package configuration
├── build.sh                # Build script
└── README.md               # Full documentation
```

## Key Features

✅ **Addition Circuit** - Proves: `z_{i+1} = z_i + gradient`  
✅ **PyO3 Bindings** - Seamless Python integration  
✅ **ProtoGalaxy Folding** - Efficient incremental verification  
✅ **Gradient Support** - Accept FL gradients as floats  
✅ **Batch Processing** - Prove multiple updates efficiently  

## Quick Start

### 1. Install Dependencies

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Python dependencies
pip install maturin
```

### 2. Build the Module

```bash
cd fl-zkp-bridge
chmod +x build.sh
./build.sh
```

Or manually:
```bash
maturin develop --release
```

### 3. Run Examples

```bash
# Basic demo
python examples/fl_demo.py

# Advanced FL demo (install torch first: pip install torch)
python examples/fl_advanced.py
```

## Usage Example

```python
import fl_zkp_bridge

# Initialize
prover = fl_zkp_bridge.FLZKPProver()
prover.initialize(0.0)

# Prove gradients from FL clients
prover.prove_gradient_step(0.5)   # Client 1
prover.prove_gradient_step(-0.3)  # Client 2
prover.prove_gradient_step(0.7)   # Client 3

# Generate proof
proof = prover.generate_final_proof()

# Verify
is_valid = prover.verify_proof(proof)
print(f"Valid: {is_valid}")  # True
```

## How It Works

### Architecture

```
┌─────────────┐
│ FL Client 1 │──┐
├─────────────┤  │
│ FL Client 2 │──┤
├─────────────┤  │   ┌──────────────┐      ┌──────────┐
│ FL Client 3 │──┼──→│ ZKP Prover   │─────→│ Verifier │
├─────────────┤  │   │ (Addition    │      │          │
│ FL Client 4 │──┤   │  Circuit)    │      └──────────┘
├─────────────┤  │   └──────────────┘
│ FL Client 5 │──┘
└─────────────┘
   Gradients        Incremental Proving    Succinct Proof
```

### Circuit Design

The addition circuit implements:
```
State Transition: z_{i+1} = z_i + gradient

Where:
- z_i: Current aggregated state
- gradient: External input from FL client
- z_{i+1}: Next state (proven correct)
```

### Cryptographic Stack

- **ProtoGalaxy**: Incremental Verifiable Computation (folding scheme)
- **BN254**: Primary elliptic curve
- **Grumpkin**: Secondary curve for CycleFold
- **Pedersen**: Commitment schemes for both curves
- **IVC Verification**: Direct verification of accumulated instances

## Integration with FL

### Workflow

1. **Initialization**: Server initializes ZKP system with initial model weights
2. **Client Training**: Each FL client computes gradients locally
3. **Gradient Proving**: Each gradient update is proven with ZKP
4. **Aggregation**: Server aggregates gradients with incremental proofs
5. **Verification**: Final proof verifies all updates are correct
6. **Model Update**: Apply verified gradients to global model

### Example Integration

```python
class FLServer:
    def __init__(self):
        self.prover = fl_zkp_bridge.FLZKPProver()
        self.prover.initialize(0.0)
    
    def aggregate_gradients(self, client_gradients):
        # Prove each gradient
        for grad in client_gradients:
            self.prover.prove_gradient_step(grad)
        
        # Generate proof of correct aggregation
        proof = self.prover.generate_final_proof()
        
        # Verify before applying
        if self.prover.verify_proof(proof):
            return self.prover.get_state()[0]
        else:
            raise Exception("Aggregation proof invalid!")
```

## API Reference

### `FLZKPProver`

| Method | Description |
|--------|-------------|
| `initialize(initial_value: float)` | Initialize with starting state |
| `prove_gradient_step(gradient: float)` | Prove single gradient update |
| `prove_gradient_batch(gradients: List[float])` | Prove batch of updates |
| `generate_final_proof()` | Generate final ZKP proof |
| `verify_proof(proof: bytes)` | Verify a proof |
| `get_state()` | Get current aggregated state |
| `get_num_steps()` | Get number of proven steps |

## Performance

| Operation | Time | Notes |
|-----------|------|-------|
| Initialization | ~3-5s | One-time setup |
| Per-gradient proving | ~10-50ms | Incremental, very fast |
| Final proof generation | ~5-10s | Produces succinct proof |
| Verification | ~5-10ms | Very fast |

## Troubleshooting

### Build Errors

**Error**: `maturin: command not found`
```bash
pip install maturin
```

**Error**: Rust compiler not found
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**Error**: `failed to compile`
Check that you're in the `fl-zkp-bridge` directory:
```bash
cd sonobe/fl-zkp-bridge
maturin develop --release
```

### Runtime Errors

**Error**: `Nova not initialized`
```python
# Always call initialize() first
prover = fl_zkp_bridge.FLZKPProver()
prover.initialize(0.0)  # ← Required!
```

## Limitations & Future Work

- [ ] **Float encoding**: Current simple scaling, need fixed-point arithmetic
- [ ] **Vector gradients**: Support multi-dimensional gradient vectors
- [ ] **Malicious security**: Add constraints for Byzantine-robust FL
- [ ] **Optimizations**: Batch proof aggregation, parallel proving
- [ ] **Privacy**: Add differential privacy integration

## Testing

```bash
# Build in debug mode for faster iteration
maturin develop

# Run basic demo
python examples/fl_demo.py

# Run advanced demo
python examples/fl_advanced.py
```

## Next Steps

1. ✅ Build the module: `./build.sh`
2. ✅ Run the basic demo: `python examples/fl_demo.py`
3. ✅ Integrate with your FL system
4. 📝 Customize the circuit for your use case
5. 🚀 Deploy to production

## Resources

- [Sonobe Documentation](https://github.com/privacy-scaling-explorations/sonobe)
- [Nova Paper](https://eprint.iacr.org/2021/370)
- [PyO3 Guide](https://pyo3.rs/)
- [Federated Learning](https://arxiv.org/abs/1602.05629)

## Support

For issues or questions:
1. Check the examples in `examples/`
2. Review the full documentation in `README.md`
3. Check Sonobe repository for ZKP-specific issues

---

**Ready to build?**
```bash
cd fl-zkp-bridge && ./build.sh && python examples/fl_demo.py
```
