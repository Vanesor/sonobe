# FL-ZKP Bridge Examples

This directory contains examples demonstrating the FL-ZKP Bridge.

## Examples

### 1. fl_demo.py
Basic demonstration of the ZKP system for gradient aggregation.

**What it demonstrates:**
- Initializing the ZKP prover
- Proving individual gradient steps
- Batch proving
- Generating and verifying final proofs

**Run:**
```bash
python examples/fl_demo.py
```

### 2. fl_advanced.py
Advanced example showing integration with PyTorch for realistic FL scenarios.

**What it demonstrates:**
- FL client with ZKP
- FL server with aggregation verification
- Integration with neural network gradients (if PyTorch available)
- Complete federated round with proof generation

**Run:**
```bash
# With PyTorch (recommended)
pip install torch
python examples/fl_advanced.py

# Without PyTorch (simulation mode)
python examples/fl_advanced.py
```

## Expected Output

### fl_demo.py
```
============================================================
Federated Learning + ZKP Demo
============================================================

1. Initializing ZKP system with initial weight: 0.0
   Result: ZKP system initialized successfully

2. Simulating 5 FL clients with gradients:
   Client 1: gradient = 0.5
   Client 2: gradient = -0.3
   Client 3: gradient = 0.7
   Client 4: gradient = 0.2
   Client 5: gradient = -0.1

3. Proving gradient updates with ZKP...
   Step 1: Step proven. Current state: 0.5
   Step 2: Step proven. Current state: 0.2
   ...

4. Final Results:
   Number of proven steps: 5
   Final aggregated weight: 1.0
   Expected (sum of gradients): 1.0

5. Generating final ZKP proof...
   Proof generated successfully! Size: XXXX bytes

6. Verifying the proof...
   Proof verification result: True

✓ SUCCESS: All gradient updates are proven and verified!
```

## Notes

- The examples use simplified gradient values for demonstration
- In production FL systems, gradients would come from actual model training
- The ZKP proves correct aggregation without revealing individual gradients
- Initialization takes a few seconds (one-time setup cost)
- Per-gradient proving is very fast (milliseconds)
- Final proof generation is more expensive but produces a succinct proof
