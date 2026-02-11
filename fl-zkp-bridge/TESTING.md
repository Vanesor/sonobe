# Testing Guide - FL-ZKP System

## What This System Does

**Zero-Knowledge Proof system for Federated Learning gradient aggregation**

- **Proves:** Multiple client gradients are correctly summed without revealing individual values
- **Privacy:** Individual gradients remain hidden (zero-knowledge)
- **Verifiable:** Anyone can verify the aggregation is correct
- **Technology:** ProtoGalaxy folding scheme from Sonobe library

**Use Case:** Privacy-preserving Federated Learning where server proves correct gradient aggregation to clients.

---

## Quick Test Commands

### 1. Rust Test (Fastest) - **GENERATES REAL ZKP**
```bash
cd /home/atharva/fizk_final_project/sonobe/fl-zkp-bridge
cargo run --release --example addition_circuit
```
**What it does:** Uses ProtoGalaxy to generate cryptographic ZK proofs for each gradient step  
**Expected:** Proves 5 gradient updates, verifies proof, shows "✓ VALID"  
**Time:** ~3 seconds  
**ZKP:** ✅ Yes - Full ProtoGalaxy IVC proof generated and verified

---

### 2. Python Basic Test - **GENERATES REAL ZKP**
```bash
cd /home/atharva/fizk_final_project/sonobe/fl-zkp-bridge
source .venv/bin/activate
python3 examples/fl_demo.py
```
**What it does:** Calls Rust ZKP system via PyO3, generates ProtoGalaxy proofs  
**Expected:** Aggregates 5 gradients, generates 800-byte proof, verifies  
**Time:** ~4 seconds  
**ZKP:** ✅ Yes - Same cryptographic proofs as Rust version

--- - **GENERATES REAL ZKP**
```bash
cd /home/atharva/fizk_final_project/sonobe/fl-zkp-bridge
source .venv/bin/activate
python3 examples/fl_advanced.py
```
**What it does:** Real PyTorch model → Real gradients → Real ZKP proofs  
**Expected:** Simulates FL model training, proves gradient aggregation  
**Time:** ~5 seconds  
**ZKP:** ✅ Yes - Proves aggregation of actual neural network gradient
**Expected:** Simulates FL model training, proves gradient aggregation  
**Time:** ~5 seconds

---

## Build Commands

### First Time Setup
```bash
# Build Rust library
cargo build --lib --release

# Setup Python environment
python3 -m venv .venv
source .venv/bin/activate

# Install Python module
maturin develop --release

# (Optional) Install PyTorch for advanced demo
pip install torch --index-url https://download.pytorch.org/whl/cpu
```

### Rebuild After Code Changes
```bash
# Rust only
cargo build --lib --release

# Python bindings
source .venv/bin/activate
maturin develop --release
```

---

## Expected Output Summary

### Rust Example
```
✓ Initialization: 2.2s
✓ Step 1-5 proven: ~200ms each
✓ Verification: 63ms
✓ Result: VALID
```

### Python Demo
```
✓ 5 clients proven
✓ Final weight: 1.0 (correct sum)
✓ Proof: 800 bytes
✓ Verified: True
```

### Advanced PyTorch Demo
```
✓ Simulated model training
✓ Computed real gradients
✓ Proved aggregation
✓ Verified: Aggregation correct
```

--- (Real ZKP!)

**Statement:** "I correctly computed z = gradient₁ + gradient₂ + ... + gradientₙ"

**Without revealing:** Individual gradient values (zero-knowledge property)

**Cryptographic Scheme:** ProtoGalaxy folding (from Sonobe library)

**Proof Components:**
- Committed instances (Pedersen commitments on BN254 curve)
- Folding witnesses (CycleFold on Grumpkin curve)
- R1CS constraint satisfaction proofs

**Proof size:** ~800 bytes (constant, regardless of number of clients)

**Verification time:** ~63ms (constant, cryptographic verificationregardless of number of clients)

**Verification time:** ~63ms (constant)

---

## Performance

| Operation | Time |
|-----------|------|
| Initialize | 2.2s |
| Prove 1 gradient | 200ms |
| Prove 5 gradients | 1s |
| Verify proof | 63ms |
| **Total (5 clients)** | **~3.2s** |
**This means:**
- ✅ ZKP was successfully generated using ProtoGalaxy
- ✅ R1CS constraints were satisfied
- ✅ Cryptographic proof passed verification
- ✅ Aggregation is proven correct WITHOUT revealing individual gradients

If you see errors, check [FIXES_APPLIED.md](FIXES_APPLIED.md)

---

## Yes, These Are REAL Zero-Knowledge Proofs!

**Not simulations.** All tests use production cryptography:

| Component | Technology |
|-----------|------------|
| Folding Scheme | ProtoGalaxy (IACR ePrint 2023/1106) |
| Primary Curve | BN254 (pairing-friendly) |
| Secondary Curve | Grumpkin (CycleFold) |
| Commitments | Pedersen (on both curves) |
| Constraints | R1CS (Rank-1 Constraint System) |
| Security | Cryptographic soundness |

**Verification is cryptographic** - not just comparing numbers!
---

## Quick Verification

All tests should output: **"✓ VALID"** or **"Proof VALID"**

If you see errors, check [FIXES_APPLIED.md](FIXES_APPLIED.md)
