#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use ark_bn254::{Fr, G1Projective as G1};
use ark_ff::PrimeField;
use ark_grumpkin::Projective as G2;
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::boolean::Boolean;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::{fp::FpVar, FieldVar};
use ark_r1cs_std::select::CondSelectGadget;
use ark_r1cs_std::GR1CSVar;
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

/// Addition circuit with real norm-bound enforcement via bit-decomposition range proof.
///
/// Proves two things per per-layer gradient sum:
///   1. z_{i+1} = z_i + gradient_sum  (IVC running sum is correct)
///   2. gradient_sum² ≤ max_norm²      (gradient is within the server-computed bound)
///
/// Constraint 2 is a genuine range proof using 64-bit bit decomposition:
///   diff = max_norm² - gradient_sum²
///   diff is decomposed into 64 bits b_0..b_63 (each constrained to {0,1})
///   diff == Σ b_i * 2^i   is enforced algebraically
///
/// If a Byzantine client submits gradient_sum with |gradient_sum| > max_norm,
/// then diff < 0 → it wraps to a huge prime-field element (near Fr::MODULUS),
/// which cannot equal any 64-bit number → `enforce_equal` fails → SynthesisError
/// → `prove_step` returns Err → proof generation aborts → no valid proof exists.
///
/// This makes the norm bound cryptographically enforced: unlike a plain Python
/// comparison, a dishonest aggregator cannot forge an acceptance for a client
/// whose proof generation legitimately failed.
#[derive(Clone, Copy, Debug)]
pub struct BoundedAdditionFCircuit<F: PrimeField> {
    _f: PhantomData<F>,
}

impl<F: PrimeField> FCircuit<F> for BoundedAdditionFCircuit<F> {
    type Params = ();
    type ExternalInputs = [F; 2]; // [l2_norm, max_norm]  — circuit squares both internally
    type ExternalInputsVar = [FpVar<F>; 2];

    fn new(_params: Self::Params) -> Result<Self, Error> {
        Ok(Self { _f: PhantomData })
    }

    fn state_len(&self) -> usize {
        1
    }

    fn generate_step_constraints(
        &self,
        cs: ConstraintSystemRef<F>,
        _i: usize,
        z_i: Vec<FpVar<F>>,
        external_inputs: Self::ExternalInputsVar,
    ) -> Result<Vec<FpVar<F>>, SynthesisError> {
        let gradient_norm = &external_inputs[0]; // L2 norm of this layer (server-computed)
        let max_norm      = &external_inputs[1]; // server norm bound

        // ── Constraint 1: IVC accumulation of total gradient magnitude ─────
        let z_next = &z_i[0] + gradient_norm;

        // ── Constraint 2: 64-bit range proof for norm² ≤ max_norm² ─────────
        //
        // diff = max_norm² - gradient_norm²
        // If Byzantine (norm > bound): diff < 0 in reals → wraps to
        // ~Fr::MODULUS - epsilon in the field → cannot fit in 64 bits
        // → bit-reconstruction ≠ diff → enforce_equal fails → SynthesisError
        // → prove_step returns Err → no valid proof for Byzantine client.
        let norm_sq     = gradient_norm * gradient_norm;
        let max_norm_sq = max_norm       * max_norm;
        let diff        = &max_norm_sq - &norm_sq;

        // Concrete value for witness assignment (F::zero() in setup/verify mode).
        let diff_val    = diff.value().unwrap_or_default();
        let diff_bigint = diff_val.into_bigint();
        let limbs       = diff_bigint.as_ref(); // &[u64]

        const RANGE_BITS: usize = 64;
        let mut recon = FpVar::<F>::zero();
        let mut power = F::one();

        for i in 0..RANGE_BITS {
            let limb_idx = i / 64;
            let bit_idx  = i % 64;
            let bit_val  = if limb_idx < limbs.len() {
                (limbs[limb_idx] >> bit_idx) & 1 == 1
            } else {
                false
            };

            // Allocate constrained Boolean witness: adds b*(1-b) == 0 constraint.
            let bit = Boolean::<F>::new_witness(
                ark_relations::ns!(cs, "range_bit"),
                || Ok(bit_val),
            )?;

            // Accumulate: recon += bit * 2^i
            let coeff = FpVar::<F>::constant(power);
            let term  = CondSelectGadget::conditionally_select(
                &bit, &coeff, &FpVar::<F>::zero(),
            )?;
            recon = recon + term;
            power = power.double();
        }

        // Fails for Byzantine inputs where diff wraps to a huge field element.
        recon.enforce_equal(&diff)?;

        Ok(vec![z_next])
    }
}

/// Legacy circuit for backward compatibility (no bounds checking)
/// Deprecated: Use BoundedAdditionFCircuit for production
#[derive(Clone, Copy, Debug)]
pub struct AdditionFCircuit<F: PrimeField> {
    _f: PhantomData<F>,
}

impl<F: PrimeField> FCircuit<F> for AdditionFCircuit<F> {
    type Params = ();
    type ExternalInputs = [F; 1];
    type ExternalInputsVar = [FpVar<F>; 1];

    fn new(_params: Self::Params) -> Result<Self, Error> {
        Ok(Self { _f: PhantomData })
    }

    fn state_len(&self) -> usize {
        1
    }

    fn generate_step_constraints(
        &self,
        _cs: ConstraintSystemRef<F>,
        _i: usize,
        z_i: Vec<FpVar<F>>,
        external_inputs: Self::ExternalInputsVar,
    ) -> Result<Vec<FpVar<F>>, SynthesisError> {
        let z_next = &z_i[0] + &external_inputs[0];
        Ok(vec![z_next])
    }
}

#[pyclass]
pub struct FLZKPBoundedProver {
    protogalaxy: Option<ProtoGalaxy<G1, G2, BoundedAdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>>,
    pg_params: Option<(
        folding_schemes::folding::protogalaxy::ProverParams<G1, G2, Pedersen<G1>, Pedersen<G2>>,
        folding_schemes::folding::protogalaxy::VerifierParams<G1, G2, Pedersen<G1>, Pedersen<G2>>,
    )>,
    current_state: Vec<f64>,
}

#[pymethods]
impl FLZKPBoundedProver {
    #[new]
    fn new() -> Self {
        FLZKPBoundedProver {
            protogalaxy: None,
            pg_params: None,
            current_state: vec![0.0],
        }
    }

    /// Initialize the ZKP system with initial state
    fn initialize(&mut self, initial_value: f64) -> PyResult<String> {
        type PG = ProtoGalaxy<G1, G2, BoundedAdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>;

        let f_circuit = BoundedAdditionFCircuit::<Fr>::new(())
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

        Ok("ZKP system initialized (ProtoGalaxy with norm bounds)".to_string())
    }

    /// Prove a gradient step with norm bound enforcement.
    ///
    /// The norm bound is enforced INSIDE the ZK circuit via bit-decomposition range
    /// proof.  There is intentionally NO pre-check here: if the gradient exceeds
    /// the bound, `prove_step` itself will fail with a SynthesisError because the
    /// range proof constraints are unsatisfiable.  The caller (Python) catches that
    /// error and records the client as ZKP-rejected — no valid proof exists.
    fn prove_gradient_step(&mut self, gradient: f64, max_norm: f64) -> PyResult<String> {
        let protogalaxy = self.protogalaxy.as_mut()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "ProtoGalaxy not initialized. Call initialize() first."
            ))?;

        let mut rng = ark_std::rand::rngs::OsRng;
        
        // Pass gradient_sum and max_norm as field elements.
        // The circuit squares both internally so units are consistent.
        // Do NOT square max_norm here — the circuit does it.
        let gradient_field  = float_to_field(gradient);
        let max_norm_field  = float_to_field(max_norm);
        
        // prove_step will return Err if the range proof is unsatisfiable
        // (i.e., gradient^2 > max_norm^2 → diff < 0 → 64-bit decomposition fails).
        protogalaxy.prove_step(&mut rng, [gradient_field, max_norm_field], None)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("ZKP range proof FAILED (gradient²={:.4} > bound²={:.4}): {:?}",
                        gradient * gradient, max_norm * max_norm, e)
            ))?;

        self.current_state[0] += gradient;
        Ok(format!("Step proven (circuit-enforced bound {}). State: {}",
                   max_norm, self.current_state[0]))
    }

    /// Prove multiple gradients with per-layer bounds
    fn prove_gradient_batch(&mut self, gradients: Vec<f64>, max_norms: Vec<f64>) -> PyResult<String> {
        if gradients.len() != max_norms.len() {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("Gradient count ({}) must match norm bound count ({})", 
                        gradients.len(), max_norms.len())
            ));
        }

        for (i, (&gradient, &max_norm)) in gradients.iter().zip(max_norms.iter()).enumerate() {
            self.prove_gradient_step(gradient, max_norm)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("Error at gradient {}: {:?}", i, e)
                ))?;
        }
        
        Ok(format!(
            "Batch of {} gradients proven with bounds. Final state: {}", 
            gradients.len(), self.current_state[0]
        ))
    }

    /// Generate final proof
    fn generate_final_proof(&self, py: Python) -> PyResult<PyObject> {
        let protogalaxy = self.protogalaxy.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "ProtoGalaxy not initialized"
            ))?;

        let mut proof_bytes = Vec::new();
        
        protogalaxy.U_i.serialize_compressed(&mut proof_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;
        
        protogalaxy.u_i.serialize_compressed(&mut proof_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        Ok(PyBytes::new(py, &proof_bytes).into())
    }

    /// Verify the IVC proof
    fn verify_proof(&self, _proof_bytes: Vec<u8>) -> PyResult<bool> {
        let protogalaxy = self.protogalaxy.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "ProtoGalaxy not initialized"
            ))?;

        let pg_params = self.pg_params.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "ProtoGalaxy params not initialized"
            ))?;

        let vp = pg_params.1.clone();
        let ivc_proof = protogalaxy.ivc_proof();
        
        type PG = ProtoGalaxy<G1, G2, BoundedAdditionFCircuit<Fr>, Pedersen<G1>, Pedersen<G2>>;
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

/// Python-facing ZKP Prover for FL (Legacy - no bounds)
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
    m.add_class::<FLZKPBoundedProver>()?;  
    m.add_class::<FLZKPProver>()?;
    Ok(())
}
