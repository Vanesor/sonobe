"""
Advanced example: Integration with PyTorch for Federated Learning + ZKP

This demonstrates a more realistic FL scenario with neural network gradients.
"""

try:
    import torch
    import torch.nn as nn
    TORCH_AVAILABLE = True
except ImportError:
    TORCH_AVAILABLE = False
    print("PyTorch not available. Install with: pip install torch")

import fl_zkp_bridge


class SimpleModel(nn.Module):
    """Simple neural network for demonstration"""
    def __init__(self):
        super().__init__()
        self.fc1 = nn.Linear(10, 5)
        self.fc2 = nn.Linear(5, 1)
    
    def forward(self, x):
        x = torch.relu(self.fc1(x))
        return self.fc2(x)


class FLClientWithZKP:
    """FL Client that generates ZKP for gradient updates"""
    
    def __init__(self, client_id):
        self.client_id = client_id
        self.model = SimpleModel() if TORCH_AVAILABLE else None
        self.prover = fl_zkp_bridge.FLZKPProver()
        self.prover.initialize(0.0)
    
    def compute_gradient(self, data, target):
        """Compute gradient on local data"""
        if not TORCH_AVAILABLE:
            # Simulate gradient
            return 0.5 - (self.client_id * 0.1)
        
        self.model.zero_grad()
        output = self.model(data)
        loss = nn.MSELoss()(output, target)
        loss.backward()
        
        # Aggregate all gradients into a single value (simplified)
        total_grad = sum(p.grad.sum().item() for p in self.model.parameters() if p.grad is not None)
        return total_grad
    
    def train_with_zkp(self, data, target):
        """Train on local data and generate ZKP"""
        gradient = self.compute_gradient(data, target)
        
        print(f"Client {self.client_id}: Gradient = {gradient:.4f}")
        
        # Prove the gradient update
        result = self.prover.prove_gradient_step(gradient)
        return gradient, result


class FLServerWithZKP:
    """FL Server that aggregates and verifies ZKPs"""
    
    def __init__(self, num_clients):
        self.num_clients = num_clients
        self.clients = [FLClientWithZKP(i) for i in range(num_clients)]
        self.global_prover = fl_zkp_bridge.FLZKPProver()
        self.global_prover.initialize(0.0)
    
    def federated_round(self):
        """Execute one round of federated learning with ZKP"""
        print(f"\n{'='*60}")
        print(f"Federated Learning Round with ZKP")
        print(f"{'='*60}\n")
        
        client_gradients = []
        
        # Each client trains locally
        for client in self.clients:
            if TORCH_AVAILABLE:
                # Random data for demonstration
                data = torch.randn(1, 10)
                target = torch.randn(1, 1)
            else:
                data, target = None, None
            
            gradient, _ = client.train_with_zkp(data, target)
            client_gradients.append(gradient)
        
        print(f"\nAggregating {len(client_gradients)} client gradients...")
        
        # Server aggregates with ZKP
        for i, grad in enumerate(client_gradients):
            result = self.global_prover.prove_gradient_step(grad)
            if i == 0:
                print(f"  {result}")
        
        # Get final aggregated state
        final_state = self.global_prover.get_state()
        expected_sum = sum(client_gradients)
        
        print(f"\nAggregation Results:")
        print(f"  Number of clients: {len(client_gradients)}")
        print(f"  Final aggregated value: {final_state[0]:.4f}")
        print(f"  Expected (sum): {expected_sum:.4f}")
        print(f"  Difference: {abs(final_state[0] - expected_sum):.10f}")
        
        # Generate and verify proof
        print(f"\nGenerating aggregation proof...")
        try:
            proof = self.global_prover.generate_final_proof()
            print(f"  Proof size: {len(proof)} bytes")
            
            print(f"\nVerifying proof...")
            is_valid = self.global_prover.verify_proof(list(proof))
            
            if is_valid:
                print(f"  ✓ Proof VALID: Aggregation is correct!")
            else:
                print(f"  ✗ Proof INVALID: Aggregation verification failed!")
            
            return is_valid
        except Exception as e:
            print(f"  ✗ Error: {e}")
            return False


def main():
    """Run the advanced FL+ZKP demo"""
    
    if not TORCH_AVAILABLE:
        print("Note: Running in simulation mode (PyTorch not available)\n")
    
    # Create FL server with 5 clients
    num_clients = 5
    server = FLServerWithZKP(num_clients)
    
    # Run federated round
    success = server.federated_round()
    
    print(f"\n{'='*60}")
    if success:
        print("FL+ZKP Round completed successfully!")
    else:
        print("FL+ZKP Round failed!")
    print(f"{'='*60}\n")


if __name__ == "__main__":
    main()
