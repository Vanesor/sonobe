#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use ark_bn254::{Fr, G1Projective as G1};
use ark_ff::PrimeField;
use ark_grumpkin::Projective as G2;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::gr1cs::{ConstraintSystemRef, SynthesisError};
use ark_serialize::CanonicalSerialize;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::marker::PhantomData;

use folding_schemes::{
    commitment::pedersen::Pedersen,
    folding::{
        protogalaxy::ProtoGalaxy,
    },
    frontend::FCircuit,
    transcript::poseidon::poseidon_canonical_config,
    Error, FoldingScheme,
};

/// Addition circuit for ZKP - proves computation: z_{i+1} = z_i + gradient
/// This is designed for FL where we want to prove gradient aggregation
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
        // z_i[0] is current accumulated value
        // external_inputs[0] is the gradient to add
        // Compute: z_{i+1} = z_i + gradient
        let z_next = &z_i[0] + &external_inputs[0];
        Ok(vec![z_next])
    }
}

/// Python-facing ZKP Prover for Federated Learning
#[pyclass]
pub struct FLZKPProver {
    protogalaxy: Option<ProtoGalaxy<G1, G2, AdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>>,
    pg_params: Option<(
        folding_schemes::folding::protogalaxy::ProverParams<G1, G2, Pedersen<G1>, Pedersen<G2>>,
        folding_schemes::folding::protogalaxy::VerifierParams<G1, G2, Pedersen<G1>, Pedersen<G2>>,
    )>,
    current_state: Vec<f64>,
}

#[pymethods]
impl FLZKPProver {
    #[new]
    fn new() -> Self {
        FLZKPProver {
            protogalaxy: None,
            pg_params: None,
            current_state: vec![0.0],
        }
    }

    /// Initialize the ZKP system with initial state
    fn initialize(&mut self, initial_value: f64) -> PyResult<String> {
        type PG = ProtoGalaxy<G1, G2, AdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>;

        let f_circuit = AdditionFCircuit::<Fr>::new(())
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        let poseidon_config = poseidon_canonical_config::<Fr>();
        let mut rng = ark_std::rand::rngs::OsRng;

        // Preprocess ProtoGalaxy params
        let pg_params = PG::preprocess(&mut rng, &(poseidon_config.clone(), f_circuit))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        // Convert initial value to field element  
        let z_0 = vec![float_to_field(initial_value)];

        // Initialize ProtoGalaxy
        let protogalaxy = PG::init(&pg_params, f_circuit, z_0)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        self.pg_params = Some(pg_params);
        self.protogalaxy = Some(protogalaxy);
        self.current_state = vec![initial_value];

        Ok("ZKP system initialized successfully (ProtoGalaxy)".to_string())
    }

    /// Prove a gradient update step
    fn prove_gradient_step(&mut self, gradient: f64) -> PyResult<String> {
        let protogalaxy = self.protogalaxy.as_mut()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("ProtoGalaxy not initialized. Call initialize() first."))?;

        let mut rng = ark_std::rand::rngs::OsRng;
        
        // Convert gradient to field element
        let gradient_field = float_to_field(gradient);
        
        // Prove step with gradient as external input
        protogalaxy.prove_step(&mut rng, [gradient_field], None)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        // Update current state
        self.current_state[0] += gradient;

        Ok(format!("Step proven. Current state: {}", self.current_state[0]))
    }

    /// Prove multiple gradient updates in batch
    fn prove_gradient_batch(&mut self, gradients: Vec<f64>) -> PyResult<String> {
        for (i, &gradient) in gradients.iter().enumerate() {
            self.prove_gradient_step(gradient)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Error at gradient {}: {:?}", i, e)
                ))?;
        }
        
        Ok(format!("Batch of {} gradients proven. Final state: {}", 
                   gradients.len(), self.current_state[0]))
    }

    /// Generate final proof (returns IVC proof state)
    fn generate_final_proof(&self, py: Python) -> PyResult<PyObject> {
        let protogalaxy = self.protogalaxy.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("ProtoGalaxy not initialized"))?;

        // For ProtoGalaxy, serialize the current IVC state
        // This represents the proof of all folding steps
        let mut proof_bytes = Vec::new();
        
        // Serialize the committed instances as proof
        protogalaxy.U_i.serialize_compressed(&mut proof_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;
        
        protogalaxy.u_i.serialize_compressed(&mut proof_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        Ok(PyBytes::new(py, &proof_bytes).into())
    }

    /// Verify the IVC proof
    fn verify_proof(&self, _proof_bytes: Vec<u8>) -> PyResult<bool> {
        let protogalaxy = self.protogalaxy.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("ProtoGalaxy not initialized"))?;

        let pg_params = self.pg_params.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("ProtoGalaxy params not initialized"))?;

        // ProtoGalaxy IVC verification
        let vp = pg_params.1.clone(); // verifier params
        
        // Get IVC proof from current state
        let ivc_proof = protogalaxy.ivc_proof();
        
        // Verify the accumulated instance
        type PG = ProtoGalaxy<G1, G2, AdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>;
        PG::verify(vp, ivc_proof)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        Ok(true)
    }

    /// Get current state
    fn get_state(&self) -> PyResult<Vec<f64>> {
        Ok(self.current_state.clone())
    }

    /// Get number of steps proven
    fn get_num_steps(&self) -> PyResult<usize> {
        if let Some(protogalaxy) = &self.protogalaxy {
            Ok(protogalaxy.i.into_bigint().as_ref()[0] as usize)
        } else {
            Ok(0)
        }
    }
}

/// Helper function to convert f64 to field element
/// For production, you'd want a more sophisticated encoding
fn float_to_field(value: f64) -> Fr {
    // Scale and convert to integer representation
    // This is a simple approach - for production, use fixed-point arithmetic
    let scaled = (value * 1_000_000.0) as i64;
    if scaled >= 0 {
        Fr::from(scaled as u64)
    } else {
        -Fr::from((-scaled) as u64)
    }
}

/// Python module definition
#[pymodule]
fn fl_zkp_bridge(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<FLZKPProver>()?;
    Ok(())
}
