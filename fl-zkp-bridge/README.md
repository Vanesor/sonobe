# FL-ZKP Bridge

A Zero-Knowledge Proof system for Federated Learning using the Sonobe library.

## Overview

This module provides PyO3 bindings for integrating Zero-Knowledge Proofs into Federated Learning workflows. It implements an **addition circuit** that proves gradient aggregation while maintaining privacy.

## Features

- **Addition Circuit**: Proves correct gradient aggregation: `z_{i+1} = z_i + gradient`
- **PyO3 Bindings**: Seamless Python integration for FL frameworks
- **ProtoGalaxy Folding Scheme**: Efficient incremental verification using IVC (Incrementally Verifiable Computation)
- **Privacy-Preserving**: Prove gradient updates without revealing individual gradients
- **Batch Processing**: Support for proving multiple gradient updates efficiently

## Architecture

```
FL Client 1 → gradient_1 ─┐
FL Client 2 → gradient_2 ─┤
FL Client 3 → gradient_3 ─┼→ ZKP Prover → Proof → Verifier
FL Client 4 → gradient_4 ─┤
FL Client 5 → gradient_5 ─┘
```

The system uses:
- **ProtoGalaxy**: Incremental Verifiable Computation folding scheme
- **Pedersen Commitment**: For both primary (BN254) and secondary curves (Grumpkin)
- **IVC Verification**: Direct verification of folded instances

## Installation

### Prerequisites

- Rust (latest stable)
- Python 3.8+
- Maturin

### Build from source

```bash
cd fl-zkp-bridge

# Install maturin
pip install maturin

# Build and install in development mode
maturin develop --release

# Or build wheel
maturin build --release
```

## Usage

### Basic Example

```python
import fl_zkp_bridge

# Initialize prover with initial model weight
prover = fl_zkp_bridge.FLZKPProver()
prover.initialize(0.0)

# Prove gradient updates from FL clients
prover.prove_gradient_step(0.5)   # Client 1
prover.prove_gradient_step(-0.3)  # Client 2
prover.prove_gradient_step(0.7)   # Client 3

# Generate final proof
proof = prover.generate_final_proof()

# Verify proof
is_valid = prover.verify_proof(proof)
print(f"Proof valid: {is_valid}")
```

### Batch Processing

```python
# Prove multiple gradients at once
gradients = [0.1, 0.2, 0.3, 0.4, 0.5]
prover.prove_gradient_batch(gradients)
```

### Integration with FL Framework

```python
# Example with PyTorch federated learning
import torch
from fl_zkp_bridge import FLZKPProver

class FLClientWithZKP:
    def __init__(self):
        self.prover = FLZKPProver()
        self.prover.initialize(0.0)
    
    def aggregate_gradients_with_proof(self, model):
        # Extract gradients
        gradients = [p.grad.sum().item() for p in model.parameters()]
        
        # Prove each gradient update
        for grad in gradients:
            self.prover.prove_gradient_step(grad)
        
        # Generate proof of correct aggregation
        proof = self.prover.generate_final_proof()
        return proof
```

## API Reference

### FLZKPProver

#### `__init__()`
Initialize a new ZKP prover instance.

#### `initialize(initial_value: float) -> str`
Initialize the ZKP system with an initial state value.

**Parameters:**
- `initial_value`: Initial weight/state value (typically 0.0)

**Returns:** Status message

#### `prove_gradient_step(gradient: float) -> str`
Prove a single gradient update step.

**Parameters:**
- `gradient`: Gradient value from an FL client

**Returns:** Status message with current state

#### `prove_gradient_batch(gradients: List[float]) -> str`
Prove multiple gradient updates in batch.

**Parameters:**
- `gradients`: List of gradient values

**Returns:** Status message with final state

#### `generate_final_proof() -> bytes`
Generate the final ZKP proof after all gradient steps.

**Returns:** Serialized proof bytes

#### `verify_proof(proof_bytes: bytes) -> bool`
Verify a generated proof.

**Parameters:**
- `proof_bytes`: Serialized proof

**Returns:** True if proof is valid, False otherwise

#### `get_state() -> List[float]`
Get current aggregated state.

**Returns:** List containing current state value

#### `get_num_steps() -> int`
Get number of proven steps.

**Returns:** Number of gradient steps proven

## Circuit Design

The addition circuit implements the state transition function:

```
z_{i+1} = z_i + gradient
```

Where:
- `z_i`: Current aggregated state
- `gradient`: Input from FL client (external input)
- `z_{i+1}`: Next state after aggregation

This proves correct gradient aggregation without revealing individual gradient values.

## Performance Considerations

- **Initialization**: One-time setup cost (~few seconds)
- **Per-step proving**: Fast incremental proving (~milliseconds per gradient)
- **Final proof generation**: More expensive (~seconds) but produces succinct proof
- **Verification**: Very fast (~milliseconds)

## Security

- Uses cryptographically secure curves (BN254, Grumpkin)
- Based on well-studied folding schemes (Nova)
- Final proof uses Groth16 for succinctness

## Examples

See `examples/fl_demo.py` for a complete demonstration.

## Testing

```bash
# Run the demo
cd fl-zkp-bridge
maturin develop --release
python examples/fl_demo.py
```

## Limitations & Future Work

1. **Float Encoding**: Current implementation uses simple scaling. Production should use proper fixed-point arithmetic.
2. **Multi-dimensional Gradients**: Currently supports single value. Extend to vectors.
3. **Malicious Security**: Add additional constraints for Byzantine-robust FL.
4. **Optimizations**: Batch proof aggregation, proof compression.

## License

MIT

## References

- [Sonobe](https://github.com/privacy-scaling-explorations/sonobe)
- [Nova: Recursive Zero-Knowledge Arguments from Folding Schemes](https://eprint.iacr.org/2021/370)
- [PyO3](https://pyo3.rs/)
