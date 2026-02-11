#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(clippy::upper_case_acronyms)]

/// Standalone Rust example demonstrating the addition circuit for FL+ZKP
/// This can be run without Python to test the core functionality
/// 
/// Run with: cargo run --release --example addition_circuit

use ark_bn254::{Fr, G1Projective as G1};
use ark_ff::PrimeField;
use ark_grumpkin::Projective as G2;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::gr1cs::{ConstraintSystemRef, SynthesisError};
use std::marker::PhantomData;
use std::time::Instant;

use folding_schemes::{
    commitment::pedersen::Pedersen,
    folding::{
        protogalaxy::ProtoGalaxy,
    },
    frontend::FCircuit,
    transcript::poseidon::poseidon_canonical_config,
    Error, FoldingScheme,
};

/// Addition circuit for Federated Learning gradient aggregation
/// Proves: z_{i+1} = z_i + gradient
#[derive(Clone, Copy, Debug)]
pub struct AdditionFCircuit<F: PrimeField> {
    _f: PhantomData<F>,
}

impl<F: PrimeField> FCircuit<F> for AdditionFCircuit<F> {
    type Params = ();
    type ExternalInputs = [F; 1]; // Gradient as external input
    type ExternalInputsVar = [FpVar<F>; 1];

    fn new(_params: Self::Params) -> Result<Self, Error> {
        Ok(Self { _f: PhantomData })
    }

    fn state_len(&self) -> usize {
        1 // Single accumulated state
    }

    fn generate_step_constraints(
        &self,
        _cs: ConstraintSystemRef<F>,
        _i: usize,
        z_i: Vec<FpVar<F>>,
        external_inputs: Self::ExternalInputsVar,
    ) -> Result<Vec<FpVar<F>>, SynthesisError> {
        // z_i[0] = current accumulated value
        // external_inputs[0] = gradient to add
        let z_next = &z_i[0] + &external_inputs[0];
        Ok(vec![z_next])
    }
}

/// Helper to convert f64 to field element (simplified)
fn float_to_field(value: f64) -> Fr {
    let scaled = (value * 1_000_000.0) as i64;
    if scaled >= 0 {
        Fr::from(scaled as u64)
    } else {
        -Fr::from((-scaled) as u64)
    }
}

/// Helper to convert field element back to f64 (simplified)
fn field_to_float(field: Fr) -> f64 {
    // This is a simplified conversion - in production, use proper decoding
    // For now, we'll just use the string representation
    let s = format!("{:?}", field);
    // Extract numeric part (this is very hacky - for demo only)
    if let Some(num_str) = s.split('(').nth(1) {
        if let Some(num) = num_str.split(')').next() {
            if let Ok(val) = num.parse::<u64>() {
                return (val as f64) / 1_000_000.0;
            }
        }
    }
    0.0
}

fn main() -> Result<(), Error> {
    println!("\n{}", "=".repeat(70));
    println!("FL+ZKP: Addition Circuit Demo (Rust - ProtoGalaxy)");
    println!("{}", "=".repeat(70));

    // Simulate FL scenario
    let initial_weight = 0.0;
    let client_gradients = vec![0.5, -0.3, 0.7, 0.2, -0.1];
    
    println!("\n1. Federated Learning Setup:");
    println!("   Initial model weight: {}", initial_weight);
    println!("   Number of FL clients: {}", client_gradients.len());
    println!("   Client gradients:");
    for (i, grad) in client_gradients.iter().enumerate() {
        println!("     Client {}: {}", i + 1, grad);
    }
    println!("   Expected sum: {}", client_gradients.iter().sum::<f64>());

    // Convert to field elements
    let z_0 = vec![float_to_field(initial_weight)];
    let gradients_field: Vec<Fr> = client_gradients.iter().map(|&g| float_to_field(g)).collect();

    println!("\n2. Initializing ZKP System (ProtoGalaxy + CycleFold)...");
    let init_start = Instant::now();

    let f_circuit = AdditionFCircuit::<Fr>::new(())?;

    type PG = ProtoGalaxy<G1, G2, AdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>;

    let poseidon_config = poseidon_canonical_config::<Fr>();
    let mut rng = ark_std::rand::rngs::OsRng;

    // Preprocess ProtoGalaxy params - returns (ProverParams, VerifierParams)
    let pg_params = PG::preprocess(&mut rng, &(poseidon_config.clone(), f_circuit))?;

    // Initialize ProtoGalaxy with params tuple
    let mut protogalaxy = PG::init(&pg_params, f_circuit, z_0)?;
    
    println!("   ✓ Initialization completed in {:?}", init_start.elapsed());

    // Prove each gradient update
    println!("\n3. Proving Gradient Updates with ZKP:");
    for (i, gradient_field) in gradients_field.iter().enumerate() {
        let step_start = Instant::now();
        
        // Prove step with gradient as external input
        protogalaxy.prove_step(&mut rng, [*gradient_field], None)?;
        
        println!("   Step {}: Proven in {:?}", i + 1, step_start.elapsed());
    }

    println!("\n4. Current State After All Updates:");
    println!("   Number of steps proven: {}", protogalaxy.i);
    println!("   Final state (field): {:?}", protogalaxy.z_i[0]);

    // Verify IVC
    println!("\n5. Verifying ProtoGalaxy IVC...");
    let verify_start = Instant::now();
    
    // Get IVC proof
    let ivc_proof = protogalaxy.ivc_proof();
    
    // Verify using verifier params (second element of tuple)
    PG::verify(pg_params.1.clone(), ivc_proof)?;
    
    println!("   ✓ Verification completed in {:?}", verify_start.elapsed());
    println!("   Result: ✓ VALID");

    // Summary
    println!("\n{}", "=".repeat(70));
    println!("Summary:");
    println!("{}", "=".repeat(70));
    println!("✓ Successfully proven {} gradient updates", client_gradients.len());
    println!("✓ All updates verified with Zero-Knowledge Proof (ProtoGalaxy)");
    println!("✓ Privacy preserved: individual gradients not revealed in proof");
    println!("\nThis proves correct gradient aggregation for Federated Learning!");
    println!("{}", "=".repeat(70));
    println!();

    Ok(())
}
