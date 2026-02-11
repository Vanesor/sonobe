#!/usr/bin/env python3
"""
Example usage of FL-ZKP Bridge for Federated Learning with Zero-Knowledge Proofs

This demonstrates how to:
1. Initialize a ZKP prover for gradient aggregation
2. Prove gradient updates from FL clients
3. Generate and verify final proofs
"""

import fl_zkp_bridge

def main():
    print("=" * 60)
    print("Federated Learning + ZKP Demo")
    print("=" * 60)
    
    # Initialize the ZKP prover with initial model weight
    prover = fl_zkp_bridge.FLZKPProver()
    initial_weight = 0.0
    
    print(f"\n1. Initializing ZKP system with initial weight: {initial_weight}")
    result = prover.initialize(initial_weight)
    print(f"   Result: {result}")
    
    # Simulate gradients from FL clients
    # In a real FL system, these would come from different clients
    client_gradients = [
        0.5,   # Client 1 gradient
        -0.3,  # Client 2 gradient
        0.7,   # Client 3 gradient
        0.2,   # Client 4 gradient
        -0.1,  # Client 5 gradient
    ]
    
    print(f"\n2. Simulating {len(client_gradients)} FL clients with gradients:")
    for i, grad in enumerate(client_gradients, 1):
        print(f"   Client {i}: gradient = {grad}")
    
    # Prove each gradient update step
    print("\n3. Proving gradient updates with ZKP...")
    for i, gradient in enumerate(client_gradients, 1):
        result = prover.prove_gradient_step(gradient)
        print(f"   Step {i}: {result}")
    
    # Get final state
    final_state = prover.get_state()
    num_steps = prover.get_num_steps()
    print(f"\n4. Final Results:")
    print(f"   Number of proven steps: {num_steps}")
    print(f"   Final aggregated weight: {final_state[0]}")
    print(f"   Expected (sum of gradients): {sum(client_gradients)}")
    
    # Generate final proof
    print("\n5. Generating final ZKP proof...")
    try:
        proof = prover.generate_final_proof()
        print(f"   Proof generated successfully! Size: {len(proof)} bytes")
        
        # Verify the proof
        print("\n6. Verifying the proof...")
        is_valid = prover.verify_proof(list(proof))
        print(f"   Proof verification result: {is_valid}")
        
        if is_valid:
            print("\n✓ SUCCESS: All gradient updates are proven and verified!")
        else:
            print("\n✗ FAILURE: Proof verification failed!")
            
    except Exception as e:
        print(f"\n✗ Error during proof generation/verification: {e}")
    
    print("\n" + "=" * 60)
    print("Demo completed!")
    print("=" * 60)


def demo_batch_proving():
    """Demonstrate batch gradient proving"""
    print("\n" + "=" * 60)
    print("Batch Gradient Proving Demo")
    print("=" * 60)
    
    prover = fl_zkp_bridge.FLZKPProver()
    prover.initialize(0.0)
    
    # Batch of gradients
    batch_gradients = [0.1, 0.2, 0.3, 0.4, 0.5]
    
    print(f"\nProving batch of {len(batch_gradients)} gradients: {batch_gradients}")
    result = prover.prove_gradient_batch(batch_gradients)
    print(f"Result: {result}")
    
    print("=" * 60)


if __name__ == "__main__":
    main()
    demo_batch_proving()
