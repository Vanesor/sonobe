#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(clippy::upper_case_acronyms)]

use ark_bn254::{Bn254, Fr, G1Affine, G1Projective as G1};
use ark_ec::CurveGroup;
use ark_ff::{PrimeField, Zero};
use ark_grumpkin::Projective as G2;
use ark_groth16::Groth16;
use ark_r1cs_std::{alloc::AllocVar, fields::{fp::FpVar, FieldVar}};
use ark_relations::gr1cs::{ConstraintSystemRef, SynthesisError};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
use folding_schemes::commitment::kzg::KZG;
use folding_schemes::commitment::pedersen::Pedersen;
use folding_schemes::folding::protogalaxy::decider_eth::Decider as ProtoGalaxyDeciderEth;
use folding_schemes::folding::protogalaxy::{IVCProof, ProtoGalaxy};
use folding_schemes::transcript::poseidon::poseidon_canonical_config;
use folding_schemes::folding::traits::CommittedInstanceOps;
use folding_schemes::Decider as DeciderTrait;
use folding_schemes::Error;
use folding_schemes::{frontend::FCircuit, FoldingScheme};
use pyo3::prelude::*;
use std::marker::PhantomData;

// ============================================================
// Model-Agnostic Gradient Fingerprint Circuit
// ============================================================
//
// Security model:
//   - Client receives global model with fingerprint F_model = <r_model, flatten(W)>
//   - Client computes gradient g after real training
//   - Client sends the FL gradient update + a ZK proof that:
//       (1) It used a model with the committed fingerprint F_model (in z_0[0])
//       (2) The gradient fingerprint <r, g> equals the accumulated value in z_i[1]
//   - Server verifies the IVC proof → accepts gradient only if proof is valid
//
// The proof is model-agnostic: it works for any gradient vector regardless of
// model architecture (linear, MLP, CNN). Only the number of IVC steps changes.
//
// CHUNK_SIZE: number of gradient elements proved per IVC step.
// 50 elements per step → minimal constraint count, reasonable step count for all models.
// Linear model (7850 params)  → ~157 steps
// MLP small (101k params)     → ~2020 steps (use sample_size instead for MLP/CNN)
// We expose a configurable chunk_size at the Python level with a max of CHUNK_SIZE elements.
//
// For large models (MLP, CNN), the Python layer samples SAMPLE_GRAD elements from the
// full gradient and proves only those — same Schwartz-Zippel security argument.

// ====================================================================
// Architecture Spec (architecture.md Section 2):
//   C = 2048 parameters per fold step.
//   Reduces IVC steps for a 7850-param linear model from ~157 → 4 steps.
//   Each step: 2048 inner-product gates + 2048 squaring gates = 4096 R1CS muls.
// ====================================================================
const CHUNK_SIZE: usize = 2048;

// ============================================================
// ExternalInputs types for GradientFingerprintCircuit
// ============================================================

/// One IVC step's external inputs: [g_chunk, r_chunk, ref_chunk] (CHUNK_SIZE each)
/// g_chunk: gradient values for this chunk
/// r_chunk: server-chosen random challenge (Fiat-Shamir)
/// ref_chunk: reference gradient chunk for directional verification
#[derive(Clone, Debug)]
pub struct GradientExternalInputs<F: PrimeField> {
    pub values: Vec<F>, // always length 3 * CHUNK_SIZE
}

impl<F: PrimeField> Default for GradientExternalInputs<F> {
    fn default() -> Self {
        Self { values: vec![F::zero(); 3 * CHUNK_SIZE] }
    }
}

#[derive(Clone, Debug)]
pub struct GradientExternalInputsVar<F: PrimeField> {
    pub values: Vec<FpVar<F>>,
}

impl<F: PrimeField> AllocVar<GradientExternalInputs<F>, F> for GradientExternalInputsVar<F> {
    fn new_variable<T: std::borrow::Borrow<GradientExternalInputs<F>>>(
        cs: impl Into<ark_relations::gr1cs::Namespace<F>>,
        f: impl FnOnce() -> Result<T, SynthesisError>,
        mode: ark_r1cs_std::alloc::AllocationMode,
    ) -> Result<Self, SynthesisError> {
        let ns = cs.into();
        let cs = ns.cs();
        let inputs = f()?;
        let inputs = inputs.borrow();
        let values = inputs
            .values
            .iter()
            .map(|v| FpVar::new_variable(cs.clone(), || Ok(*v), mode))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { values })
    }
}

// ============================================================
// GradientFingerprintCircuit  (architecture.md Section 2)
// ============================================================
/// Model-agnostic IVC circuit — proves gradient commitment AND accumulates L2 norm².
///
/// State z = [model_fp, ref_grad_fp, grad_fp_accum, norm_sq_accum, directional_fp_accum, step_count]  (6 elements)
///
/// Per step (CHUNK_SIZE = 2048 elements):
///   g_chunk   = external_inputs[0..CHUNK_SIZE]              (quantized gradient slice)
///   r_chunk   = external_inputs[CHUNK_SIZE..2*CHUNK_SIZE]   (Fiat-Shamir challenge)
///   ref_chunk = external_inputs[2*CHUNK_SIZE..3*CHUNK_SIZE] (quantized reference slice)
///
///   Fingerprint constraint (2048 mul gates):
///     dot = Σ r_j * g_j  →  grad_fp_accum' = grad_fp_accum + dot
///
///   Norm² accumulation constraint (2048 mul gates):
///     norm_sq_accum' = norm_sq_accum + Σ g_j²
///
///   Directional accumulation constraint (2048 mul gates):
///     directional_fp_accum' = directional_fp_accum + Σ g_j * ref_j
///
/// The final state accumulations are embedded in the IVC proof public output.
/// The Python server checks: norm_sq_accum ≤ B_l² (EMA-based bound) and directional_fp_accum >= 0.
/// Clients cannot fake this value without breaking the ZK proof.
#[derive(Clone, Debug)]
pub struct GradientFingerprintCircuit<F: PrimeField> {
    _f: PhantomData<F>,
}

impl<F: PrimeField> FCircuit<F> for GradientFingerprintCircuit<F> {
    type Params = ();
    type ExternalInputs = GradientExternalInputs<F>;
    type ExternalInputsVar = GradientExternalInputsVar<F>;

    fn new(_params: Self::Params) -> Result<Self, Error> {
        Ok(Self { _f: PhantomData })
    }

    fn state_len(&self) -> usize {
        7 // [model_fp, ref_grad_fp, grad_fp_accum, norm_sq_accum, directional_fp_accum, ref_fp_accum, step_count]
    }

    fn generate_step_constraints(
        &self,
        _cs: ConstraintSystemRef<F>,
        _i: usize,
        z_i: Vec<FpVar<F>>,
        external_inputs: Self::ExternalInputsVar,
    ) -> Result<Vec<FpVar<F>>, SynthesisError> {
        let ext = &external_inputs.values;

        let model_fp             = &z_i[0];
        let ref_grad_fp          = &z_i[1];
        let grad_fp_accum        = &z_i[2];
        let norm_sq_accum        = &z_i[3];
        let directional_fp_accum = &z_i[4];
        let ref_fp_accum         = &z_i[5];
        let step_count           = &z_i[6];

        // Unpack external inputs: [g_chunk | r_chunk | ref_chunk]
        let g_chunk   = &ext[0..CHUNK_SIZE];
        let r_chunk   = &ext[CHUNK_SIZE..2 * CHUNK_SIZE];
        let ref_chunk = &ext[2 * CHUNK_SIZE..3 * CHUNK_SIZE];

        // === CONSTRAINTS ===
        let mut dot = FpVar::zero();
        let mut norm_sq_delta = FpVar::zero();
        let mut directional_delta = FpVar::zero();
        let mut ref_fp_delta = FpVar::zero();
        for j in 0..CHUNK_SIZE {
            // Fingerprint term: r_j * g_j
            let r_times_g = &r_chunk[j] * &g_chunk[j];
            dot = &dot + &r_times_g;

            // Norm² term: g_j * g_j
            let g_squared = &g_chunk[j] * &g_chunk[j];
            norm_sq_delta = &norm_sq_delta + &g_squared;

            // Directional term: g_j * ref_j
            let g_times_ref = &g_chunk[j] * &ref_chunk[j];
            directional_delta = &directional_delta + &g_times_ref;

            // Reference fingerprint spoof prevention term: r_j * ref_j
            let r_times_ref = &r_chunk[j] * &ref_chunk[j];
            ref_fp_delta = &ref_fp_delta + &r_times_ref;
        }

        // === STATE TRANSITION ===
        let new_grad_fp_accum        = grad_fp_accum + &dot;
        let new_norm_sq_accum        = norm_sq_accum + &norm_sq_delta;
        let new_directional_fp_accum = directional_fp_accum + &directional_delta;
        let new_ref_fp_accum         = ref_fp_accum + &ref_fp_delta;
        let new_step_count           = step_count + &FpVar::one();

        // model_fp and ref_grad_fp passthrough
        Ok(vec![
            model_fp.clone(),
            ref_grad_fp.clone(),
            new_grad_fp_accum,
            new_norm_sq_accum,
            new_directional_fp_accum,
            new_ref_fp_accum,
            new_step_count,
        ])
    }
}

// ============================================================
// Type aliases for the ProtoGalaxy instantiation
// ============================================================
/// KZG on BN254 is required for Sonobe's ProtoGalaxy `DeciderEth` (Groth16) final layer.
type CS1 = KZG<'static, Bn254>;
type CS2 = Pedersen<G2>;
type PGFC = GradientFingerprintCircuit<Fr>;
type PGGrad = ProtoGalaxy<G1, G2, PGFC, CS1, CS2>;
type PGGradIVCProof = IVCProof<G1, G2>;
type PGDec = ProtoGalaxyDeciderEth<G1, G2, PGFC, CS1, CS2, Groth16<Bn254>, PGGrad>;
type PgProverParams = folding_schemes::folding::protogalaxy::ProverParams<G1, G2, CS1, CS2>;
type PgVerifierParams = folding_schemes::folding::protogalaxy::VerifierParams<G1, G2, CS1, CS2>;
type DecProverParams = <PGDec as DeciderTrait<G1, G2, PGFC, PGGrad>>::ProverParam;
type DecVerifierParams = <PGDec as DeciderTrait<G1, G2, PGFC, PGGrad>>::VerifierParam;
type DecProof = <PGDec as DeciderTrait<G1, G2, PGFC, PGGrad>>::Proof;

/// Decider-finalized bundle (Groth16 + KZG openings): **O(1)** verifier work in IVC length.
const DEC_MAGIC: &[u8; 4] = b"PGD1";
const DEC_VERSION: u8 = 1;

/// Batch envelope for multiple **independent** client bundles (each in standard wire format).
/// ProtoGalaxy IVC does not merge unrelated client proofs into one succinct proof without a
/// dedicated outer recursive circuit; this format packs N verified bundles so a verifier runs
/// `PGGrad::verify` on each (O(N) crypto).
const BATCH_MAGIC: &[u8; 4] = b"PGFB";
const BATCH_VERSION: u8 = 1;

/// Verifies one client bundle: `[chunk_size u32][proof_len u32][ivc proof][vp bytes]`.
fn verify_standard_client_bundle_bytes(bundle: &[u8]) -> Result<(), String> {
    if bundle.len() < 8 {
        return Err("bundle too short".into());
    }
    let chunk_size_in_bundle = u32::from_le_bytes(bundle[0..4].try_into().unwrap()) as usize;
    if chunk_size_in_bundle != CHUNK_SIZE {
        return Err(format!(
            "chunk_size {} != compiled CHUNK_SIZE {}",
            chunk_size_in_bundle, CHUNK_SIZE
        ));
    }
    let proof_len = u32::from_le_bytes(bundle[4..8].try_into().unwrap()) as usize;
    if bundle.len() < 8 + proof_len {
        return Err("bundle truncated (proof)".into());
    }
    let proof_bytes_slice = &bundle[8..8 + proof_len];
    let vp_bytes_slice = &bundle[8 + proof_len..];

    let ivc_proof = PGGradIVCProof::deserialize_compressed(proof_bytes_slice)
        .map_err(|e| format!("IVC proof deserialize: {:?}", e))?;

    let final_state = ivc_proof.z_i.clone();
    if final_state.len() < 7 {
        return Err("IVC state vector too short".into());
    }
    let ref_fp_accum_str = format!("{}", final_state[5].into_bigint());
    let expected_ref_fp_str = format!("{}", ivc_proof.z_0[1].into_bigint());
    if ref_fp_accum_str != expected_ref_fp_str {
        return Err(format!(
            "reference fingerprint mismatch (accum {} vs z_0[1] {})",
            ref_fp_accum_str, expected_ref_fp_str
        ));
    }

    let vp = PGGrad::vp_deserialize_with_mode(
        vp_bytes_slice,
        Compress::Yes,
        Validate::Yes,
        (),
    )
    .map_err(|e| format!("VerifierParams: {:?}", e))?;

    PGGrad::verify(vp, ivc_proof).map_err(|e| format!("PG::verify: {:?}", e))?;
    Ok(())
}

fn encode_proof_batches(sub_bundles: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(BATCH_MAGIC);
    out.push(BATCH_VERSION);
    out.extend_from_slice(&[0u8; 3]);
    out.extend_from_slice(&(sub_bundles.len() as u32).to_le_bytes());
    for b in sub_bundles {
        let len = b.len();
        if len > u32::MAX as usize {
            panic!("single bundle length overflow");
        }
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(b);
    }
    out
}

fn decode_and_verify_batch_bundle(batch: &[u8]) -> Result<usize, String> {
    if batch.len() < 12 {
        return Err("batch too short".into());
    }
    if &batch[0..4] != BATCH_MAGIC {
        return Err("missing PGFB magic (not a batch bundle)".into());
    }
    if batch[4] != BATCH_VERSION {
        return Err(format!("unsupported batch version {}", batch[4]));
    }
    let count = u32::from_le_bytes(batch[8..12].try_into().unwrap()) as usize;
    let mut offset = 12usize;
    for i in 0..count {
        if offset + 4 > batch.len() {
            return Err(format!("truncated batch entry {} (length)", i));
        }
        let slen = u32::from_le_bytes(batch[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if offset + slen > batch.len() {
            return Err(format!("truncated batch entry {} (payload)", i));
        }
        let sub = &batch[offset..offset + slen];
        verify_any_client_bundle_bytes(sub)
            .map_err(|e| format!("sub-bundle {}: {}", i, e))?;
        offset += slen;
    }
    if offset != batch.len() {
        return Err("trailing bytes after batch".into());
    }
    Ok(count)
}

fn push_len_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
}

fn pop_len_bytes<'a>(bytes: &'a [u8], offset: &mut usize) -> Result<&'a [u8], String> {
    if *offset + 4 > bytes.len() {
        return Err("truncated length prefix".into());
    }
    let n = u32::from_le_bytes(bytes[*offset..*offset + 4].try_into().unwrap()) as usize;
    *offset += 4;
    if *offset + n > bytes.len() {
        return Err("truncated payload".into());
    }
    let slice = &bytes[*offset..*offset + n];
    *offset += n;
    Ok(slice)
}

fn push_fr_vec(buf: &mut Vec<u8>, v: &[Fr]) {
    buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for fr in v {
        let mut t = Vec::new();
        fr.serialize_compressed(&mut t)
            .expect("Fr serialize");
        push_len_bytes(buf, &t);
    }
}

fn pop_fr_vec(bytes: &[u8], offset: &mut usize) -> Result<Vec<Fr>, String> {
    if *offset + 4 > bytes.len() {
        return Err("truncated Fr vec count".into());
    }
    let n = u32::from_le_bytes(bytes[*offset..*offset + 4].try_into().unwrap()) as usize;
    *offset += 4;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let blob = pop_len_bytes(bytes, offset)?;
        let fr = Fr::deserialize_compressed(blob)
            .map_err(|e| format!("Fr deserialize: {:?}", e))?;
        out.push(fr);
    }
    Ok(out)
}

fn push_g1_vec(buf: &mut Vec<u8>, v: &[G1]) {
    buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for p in v {
        let mut t = Vec::new();
        p.into_affine()
            .serialize_compressed(&mut t)
            .expect("G1 serialize");
        push_len_bytes(buf, &t);
    }
}

fn pop_g1_vec(bytes: &[u8], offset: &mut usize) -> Result<Vec<G1>, String> {
    if *offset + 4 > bytes.len() {
        return Err("truncated G1 vec count".into());
    }
    let n = u32::from_le_bytes(bytes[*offset..*offset + 4].try_into().unwrap()) as usize;
    *offset += 4;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let blob = pop_len_bytes(bytes, offset)?;
        let aff = G1Affine::deserialize_compressed(blob)
            .map_err(|e| format!("G1 deserialize: {:?}", e))?;
        out.push(aff.into());
    }
    Ok(out)
}

fn parse_decider_bundle(
    bytes: &[u8],
) -> Result<
    (
        DecProof,
        DecVerifierParams,
        Fr,
        Vec<Fr>,
        Vec<Fr>,
        Vec<G1>,
        Vec<G1>,
    ),
    String,
> {
    if bytes.len() < 16 {
        return Err("decider bundle too short".into());
    }
    if &bytes[0..4] != DEC_MAGIC {
        return Err("not a decider bundle".into());
    }
    if bytes[4] != DEC_VERSION {
        return Err(format!("unsupported decider version {}", bytes[4]));
    }
    let mut off = 8usize;
    let chunk = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
    off += 4;
    if chunk != CHUNK_SIZE {
        return Err(format!("chunk marker {} != {}", chunk, CHUNK_SIZE));
    }
    let proof_blob = pop_len_bytes(bytes, &mut off)?;
    let dec_proof = DecProof::deserialize_compressed(proof_blob)
        .map_err(|e| format!("Decider proof: {:?}", e))?;
    let vp_blob = pop_len_bytes(bytes, &mut off)?;
    let dec_vp = DecVerifierParams::deserialize_compressed(vp_blob)
        .map_err(|e| format!("Decider vp: {:?}", e))?;
    let i_blob = pop_len_bytes(bytes, &mut off)?;
    let i = Fr::deserialize_compressed(i_blob).map_err(|e| format!("i: {:?}", e))?;
    let z_0 = pop_fr_vec(bytes, &mut off)?;
    let z_i = pop_fr_vec(bytes, &mut off)?;
    let run_c = pop_g1_vec(bytes, &mut off)?;
    let inc_c = pop_g1_vec(bytes, &mut off)?;
    if off != bytes.len() {
        return Err("trailing bytes in decider bundle".into());
    }
    Ok((dec_proof, dec_vp, i, z_0, z_i, run_c, inc_c))
}

fn verify_decider_bundle_crypto(bytes: &[u8]) -> Result<(String, String), String> {
    let (dec_proof, dec_vp, i, z_0, z_i, run_c, inc_c) = parse_decider_bundle(bytes)?;
    if z_i.len() < 7 || z_0.len() < 7 {
        return Err("z_i / z_0 too short".into());
    }
    let norm_sq_str = format!("{:?}", z_i[3]);
    let dir_fp_str = format!("{:?}", z_i[4]);
    let ref_fp_accum_str = format!("{}", z_i[5].into_bigint());
    let expected_ref_fp_str = format!("{}", z_0[1].into_bigint());
    if ref_fp_accum_str != expected_ref_fp_str {
        return Err(format!(
            "reference fingerprint mismatch (accum {} vs z_0[1] {})",
            ref_fp_accum_str, expected_ref_fp_str
        ));
    }
    let ok = PGDec::verify(dec_vp, i, z_0, z_i, &run_c, &inc_c, &dec_proof)
        .map_err(|e| format!("Decider verify: {:?}", e))?;
    if !ok {
        return Err("Decider verify returned false".into());
    }
    Ok((norm_sq_str, dir_fp_str))
}

fn verify_any_client_bundle_bytes(b: &[u8]) -> Result<(), String> {
    if b.len() >= 4 && &b[0..4] == DEC_MAGIC {
        verify_decider_bundle_crypto(b).map(|_| ())
    } else {
        verify_standard_client_bundle_bytes(b)
    }
}

// ============================================================
// GradientZKProver — Python-facing proof-of-gradient prover
// ============================================================
/// Generates and verifies ZK proofs that gradient chunks match a committed fingerprint.
///
/// Usage (prover side):
///   prover = GradientZKProver()
///   prover.initialize(model_fp)
///   for chunk in gradient_chunks:
///       prover.prove_chunk(g_chunk, r_chunk)
///   bundle = prover.generate_proof_bundle()   # bytes to send to server
///
/// Usage (verifier side — server):
///   ok = GradientZKProver.verify_proof_bundle_static(bundle)
///   # Returns True only if PG::verify passes (real cryptographic check)
#[pyclass]
pub struct GradientZKProver {
    protogalaxy: Option<PGGrad>,
    pg_params: Option<(PgProverParams, PgVerifierParams)>,
    decider_pp: Option<DecProverParams>,
    decider_vp: Option<DecVerifierParams>,
    vp_serialized: Vec<u8>, // serialized cs_vp + cf_cs_vp (IVC / compatibility)
    num_steps: usize,
    model_fp: i64,
    ref_grad_fp: i64,
}

#[pymethods]
impl GradientZKProver {
    #[new]
    fn new() -> Self {
        GradientZKProver {
            protogalaxy: None,
            pg_params: None,
            decider_pp: None,
            decider_vp: None,
            vp_serialized: Vec::new(),
            num_steps: 0,
            model_fp: 0,
            ref_grad_fp: 0,
        }
    }

    /// Initialize the IVC prover with a committed model fingerprint.
    ///
    /// model_fp: integer fingerprint of the global model
    /// ref_grad_fp: integer fingerprint of the reference public gradient
    ///
    /// This runs ProtoGalaxy::preprocess (expensive ~10-60s depending on hardware).
    fn initialize(&mut self, model_fp: i64, ref_grad_fp: i64) -> PyResult<String> {
        self.model_fp = model_fp;
        self.ref_grad_fp = ref_grad_fp;
        let fp_field = int_to_field(model_fp);
        let ref_fp_field = int_to_field(ref_grad_fp);
        let f_circuit = GradientFingerprintCircuit::<Fr>::new(())
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        let mut rng = ark_std::rand::rngs::OsRng;
        let poseidon_config = poseidon_canonical_config::<Fr>();
        let pg_params = PGGrad::preprocess(&mut rng, &(poseidon_config, f_circuit.clone()))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        let state_len = f_circuit.state_len();
        let (decider_pp, decider_vp) = PGDec::preprocess(&mut rng, (pg_params.clone(), state_len))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Decider preprocess: {:?}", e)))?;
        self.decider_pp = Some(decider_pp);
        self.decider_vp = Some(decider_vp);

        // Serialize vp (only cs_vp + cf_cs_vp) for the proof bundle
        let mut vp_bytes = Vec::new();
        pg_params.1.cs_vp.serialize_compressed(&mut vp_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("VP serialize: {:?}", e)))?;
        pg_params.1.cf_cs_vp.serialize_compressed(&mut vp_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("CF-VP serialize: {:?}", e)))?;
        self.vp_serialized = vp_bytes;

        // 7-element initial state
        let z_0 = vec![fp_field, ref_fp_field, Fr::zero(), Fr::zero(), Fr::zero(), Fr::zero(), Fr::zero()];
        let protogalaxy = PGGrad::init(&pg_params, f_circuit, z_0)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{:?}", e)))?;

        self.protogalaxy = Some(protogalaxy);
        self.pg_params = Some(pg_params);
        self.num_steps = 0;

        Ok(format!(
            "GradientZKProver initialized (ProtoGalaxy+KZG + DeciderEth). model_fp={}, ref_grad_fp={}, chunk_size={}",
            model_fp, ref_grad_fp, CHUNK_SIZE
        ))
    }

    /// Prove one chunk of gradient elements.
    ///
    /// g_chunk: CHUNK_SIZE gradient values (float64, scaled internally)
    /// r_chunk: CHUNK_SIZE random challenge values (float64, from server Fiat-Shamir)
    /// ref_chunk: CHUNK_SIZE reference public values (float64, scaled internally)
    ///
    /// This appends one IVC step. The circuit computes dot = <r_chunk, g_chunk>
    /// and accumulates it into grad_fp_accum.
    fn prove_chunk(&mut self, g_chunk: Vec<i64>, r_chunk: Vec<i64>, ref_chunk: Vec<i64>) -> PyResult<String> {
        if g_chunk.len() != CHUNK_SIZE {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("g_chunk must have {} elements, got {}", CHUNK_SIZE, g_chunk.len())
            ));
        }
        if r_chunk.len() != CHUNK_SIZE {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("r_chunk must have {} elements, got {}", CHUNK_SIZE, r_chunk.len())
            ));
        }
        if ref_chunk.len() != CHUNK_SIZE {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("ref_chunk must have {} elements, got {}", CHUNK_SIZE, ref_chunk.len())
            ));
        }

        let protogalaxy = self.protogalaxy.as_mut()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Not initialized. Call initialize() first."
            ))?;

        let mut ext_values = Vec::with_capacity(3 * CHUNK_SIZE);
        for &v in g_chunk.iter() {
            ext_values.push(int_to_field(v));
        }
        for &v in r_chunk.iter() {
            ext_values.push(int_to_field(v));
        }
        for &v in ref_chunk.iter() {
            ext_values.push(int_to_field(v));
        }

        let ext_inputs = GradientExternalInputs { values: ext_values };
        let mut rng = ark_std::rand::rngs::OsRng;

        protogalaxy.prove_step(&mut rng, ext_inputs, None)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("IVC prove_step failed at chunk {}: {:?}", self.num_steps, e)
            ))?;

        self.num_steps += 1;
        Ok(format!("Chunk {} proven. grad_fp_accum updated.", self.num_steps))
    }

    /// Generate a self-contained proof bundle that can be verified by any party.
    ///
    /// **Decider bundle (preferred, `PGD1` magic)** after at least two `prove_chunk` calls:
    /// Groth16 + KZG decider over the finished ProtoGalaxy IVC — verifier work is **O(1)** in the
    /// number of folding steps (IVC proof size), at the cost of heavier prover time. After a
    /// successful decider bundle, the internal IVC state is consumed; call `initialize` again for
    /// a new proof chain.
    ///
    /// **Legacy IVC bundle** when only one step was proven: Sonobe's `DeciderEth::verify` requires
    /// `i > 1`, so we fall back to embedding the raw IVC proof:
    /// `[4B: CHUNK_SIZE][4B: proof_len][ivc_proof][vp bytes]`.
    fn generate_proof_bundle(&mut self) -> PyResult<Vec<u8>> {
        self.protogalaxy.as_ref().ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Not initialized. Call initialize() first.",
            )
        })?;

        if self.num_steps == 0 {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "No steps proven yet. Call prove_chunk() at least once.",
            ));
        }

        if self.num_steps >= 2 {
            let dec_pp = self
                .decider_pp
                .as_ref()
                .ok_or_else(|| {
                    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                        "Decider params missing after initialize()",
                    )
                })?
                .clone();
            let pg_backup = self.protogalaxy.as_ref().unwrap().clone();
            let pg = self.protogalaxy.take().ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("internal: protogalaxy")
            })?;
            let i = pg.i;
            let z_0 = pg.z_0.clone();
            let z_i = pg.z_i.clone();
            let run_c = pg.U_i.get_commitments();
            let inc_c = pg.u_i.get_commitments();
            let mut rng = ark_std::rand::rngs::OsRng;
            let dec_proof = match PGDec::prove(&mut rng, dec_pp, pg) {
                Ok(p) => p,
                Err(e) => {
                    self.protogalaxy = Some(pg_backup);
                    return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                        "Decider prove: {:?}",
                        e
                    )));
                }
            };
            let dec_vp = self.decider_vp.as_ref().ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>("Decider vp missing")
            })?;

            let mut out = Vec::new();
            out.extend_from_slice(DEC_MAGIC);
            out.push(DEC_VERSION);
            out.extend_from_slice(&[0u8; 3]);
            out.extend_from_slice(&(CHUNK_SIZE as u32).to_le_bytes());

            let mut dp = Vec::new();
            dec_proof.serialize_compressed(&mut dp).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "decider proof serialize: {:?}",
                    e
                ))
            })?;
            push_len_bytes(&mut out, &dp);

            let mut dv = Vec::new();
            dec_vp.serialize_compressed(&mut dv).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "decider vp serialize: {:?}",
                    e
                ))
            })?;
            push_len_bytes(&mut out, &dv);

            let mut i_ser = Vec::new();
            i.serialize_compressed(&mut i_ser).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("i serialize: {:?}", e))
            })?;
            push_len_bytes(&mut out, &i_ser);

            push_fr_vec(&mut out, &z_0);
            push_fr_vec(&mut out, &z_i);
            push_g1_vec(&mut out, &run_c);
            push_g1_vec(&mut out, &inc_c);

            Ok(out)
        } else {
            let protogalaxy = self.protogalaxy.as_ref().unwrap();
            let ivc_proof = protogalaxy.ivc_proof();
            let mut proof_bytes = Vec::new();
            ivc_proof.serialize_compressed(&mut proof_bytes).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "IVC proof serialize: {:?}",
                    e
                ))
            })?;

            let mut bundle = Vec::new();
            bundle.extend_from_slice(&(CHUNK_SIZE as u32).to_le_bytes());
            bundle.extend_from_slice(&(proof_bytes.len() as u32).to_le_bytes());
            bundle.extend_from_slice(&proof_bytes);
            bundle.extend_from_slice(&self.vp_serialized);

            Ok(bundle)
        }
    }

    /// Verify a proof bundle using the stored VerifierParams (same process).
    ///
    /// Deserializes the IVC proof from the bundle, then calls PG::verify(vp, ivc_proof).
    /// This is a REAL cryptographic verification — not a re-proof.
    fn verify_proof_bundle(&self, bundle: Vec<u8>) -> PyResult<bool> {
        if bundle.len() >= 4 && &bundle[0..4] == DEC_MAGIC {
            return verify_decider_bundle_crypto(&bundle)
                .map(|_| true)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e));
        }

        if bundle.len() < 8 {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Bundle too short"));
        }

        let chunk_size_in_bundle = u32::from_le_bytes(bundle[0..4].try_into().unwrap()) as usize;
        if chunk_size_in_bundle != CHUNK_SIZE {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("Bundle chunk_size {} != compiled CHUNK_SIZE {}", chunk_size_in_bundle, CHUNK_SIZE)
            ));
        }

        let proof_len = u32::from_le_bytes(bundle[4..8].try_into().unwrap()) as usize;
        if bundle.len() < 8 + proof_len {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Bundle truncated"));
        }

        let proof_bytes = &bundle[8..8 + proof_len];

        // Deserialize IVC proof
        let ivc_proof = PGGradIVCProof::deserialize_compressed(proof_bytes)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("IVC proof deserialize failed (tampered?): {:?}", e)
            ))?;

        // Use stored VerifierParams (from initialize())
        let vp = self.pg_params.as_ref()
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Prover not initialized. Call initialize() first."
            ))?.1.clone();

        // REAL cryptographic verification
        PGGrad::verify(vp, ivc_proof)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("Proof verification FAILED: {:?}", e)
            ))?;

        Ok(true)
    }

    /// Standalone static verifier: verifies a proof bundle without any pre-initialized state.
    /// Reconstructs VerifierParams from the VP bytes embedded in the bundle.
    ///
    /// This is the "server-side" verification path when the verifier doesn't have
    /// access to the prover's pg_params.
    /// Returns: (is_valid, norm_sq_accum_str, directional_fp_accum_str, error_msg)
    #[staticmethod]
    fn verify_proof_bundle_static(bundle: Vec<u8>) -> PyResult<(bool, String, String, String)> {
        if bundle.len() >= 4 && &bundle[0..4] == DEC_MAGIC {
            return match verify_decider_bundle_crypto(&bundle) {
                Ok((norm_sq_str, dir_fp_str)) => Ok((true, norm_sq_str, dir_fp_str, "Valid".into())),
                Err(e) => Ok((false, String::new(), String::new(), e)),
            };
        }

        if bundle.len() < 8 {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Bundle too short"));
        }

        let chunk_size_in_bundle = u32::from_le_bytes(bundle[0..4].try_into().unwrap()) as usize;
        if chunk_size_in_bundle != CHUNK_SIZE {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("Bundle chunk_size {} != compiled CHUNK_SIZE {}", chunk_size_in_bundle, CHUNK_SIZE)
            ));
        }

        let proof_len = u32::from_le_bytes(bundle[4..8].try_into().unwrap()) as usize;
        if bundle.len() < 8 + proof_len {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>("Bundle truncated"));
        }

        let proof_bytes_slice = &bundle[8..8 + proof_len];
        let vp_bytes_slice = &bundle[8 + proof_len..];

        // Deserialize IVC proof
        let ivc_proof = PGGradIVCProof::deserialize_compressed(proof_bytes_slice)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("IVC proof deserialize failed (tampered?): {:?}", e)
            ))?;

        // Extract the final state from the IVC proof before it gets consumed
        let final_state = ivc_proof.z_i.clone();
        if final_state.len() < 7 {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "IVC proof state length is too short to extract directional constraints"
            ));
        }
        let norm_sq_str = format!("{:?}", final_state[3]);
        let dir_fp_str = format!("{:?}", final_state[4]);

        let ref_fp_accum_str = format!("{}", final_state[5].into_bigint());
        let expected_ref_fp_str = format!("{}", ivc_proof.z_0[1].into_bigint());
        if ref_fp_accum_str != expected_ref_fp_str {
            return Ok((
                false, 
                norm_sq_str, 
                dir_fp_str, 
                format!("Spoofed reference gradient detected. Circuit accumulated: {}, Init z_0[1]: {}", ref_fp_accum_str, expected_ref_fp_str)
            ));
        }

        // Reconstruct VerifierParams via vp_deserialize_with_mode.
        // This re-generates r1cs and cf_r1cs from the GradientFingerprintCircuit (params = ())
        // and deserializes cs_vp + cf_cs_vp from the bundle's vp bytes.
        let vp = PGGrad::vp_deserialize_with_mode(
            vp_bytes_slice,
            Compress::Yes,
            Validate::Yes,
            (), // GradientFingerprintCircuit::Params = ()
        ).map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("VerifierParams reconstruct failed: {:?}", e)
        ))?;

        // REAL cryptographic verification — PG::verify checks all polynomial identities
        PGGrad::verify(vp, ivc_proof)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                format!("Proof verification FAILED: {:?}", e)
            ))?;

        Ok((true, norm_sq_str, dir_fp_str, "Valid".to_string()))
    }

    /// Packs multiple **valid** client bundles into one batch blob after running full
    /// `PGGrad::verify` on each. This is not a single succinct proof; it is a canonical
    /// container for batched verification.
    #[staticmethod]
    fn fold_proofs_bundle(bundles: Vec<Vec<u8>>) -> PyResult<Vec<u8>> {
        if bundles.is_empty() {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "No proofs to fold",
            ));
        }
        for (i, b) in bundles.iter().enumerate() {
            verify_any_client_bundle_bytes(b).map_err(|e| {
                PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "Invalid sub-bundle {}: {}",
                    i, e
                ))
            })?;
        }
        Ok(encode_proof_batches(&bundles))
    }

    /// Verifies a batch produced by `fold_proofs_bundle`.
    #[staticmethod]
    fn verify_batch_bundle_static(batch: Vec<u8>) -> PyResult<(bool, String)> {
        match decode_and_verify_batch_bundle(&batch) {
            Ok(n) => Ok((true, format!("Verified {} independent IVC proofs", n))),
            Err(e) => Ok((false, e)),
        }
    }

    fn get_num_steps(&self) -> PyResult<usize> {
        Ok(self.num_steps)
    }

    fn get_model_fp(&self) -> PyResult<i64> {
        Ok(self.model_fp)
    }

    fn get_chunk_size(&self) -> PyResult<usize> {
        Ok(CHUNK_SIZE)
    }

    /// Returns current accumulated fingerprint value (for debugging)
    fn get_grad_fp_accum(&self) -> PyResult<String> {
        if let Some(pg) = &self.protogalaxy {
            let z_i = &pg.z_i;
            if z_i.len() >= 3 {
                return Ok(format!("{:?}", z_i[2]));
            }
        }
        Ok("not initialized".to_string())
    }

    /// Returns current accumulated norm² value (for server-side EMA bound check)
    fn get_norm_sq_accum(&self) -> PyResult<String> {
        if let Some(pg) = &self.protogalaxy {
            let z_i = &pg.z_i;
            if z_i.len() >= 4 {
                return Ok(format!("{:?}", z_i[3]));
            }
        }
        Ok("not initialized".to_string())
    }

    /// Returns current accumulated directional value
    fn get_directional_fp_accum(&self) -> PyResult<String> {
        if let Some(pg) = &self.protogalaxy {
            let z_i = &pg.z_i;
            if z_i.len() >= 5 {
                return Ok(format!("{:?}", z_i[4]));
            }
        }
        Ok("not initialized".to_string())
    }
}

// ============================================================
// Utilities
// ============================================================

/// Convert i64 to BN254 field element.
/// Replaces the floating point cast to maintain cryptograhic precision boundaries.
pub fn int_to_field(value: i64) -> Fr {
    if value >= 0 {
        Fr::from(value as u64)
    } else {
        -Fr::from((-value) as u64)
    }
}

// ============================================================
// Python module
// ============================================================

#[pymodule]
fn fl_zkp_bridge(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<GradientZKProver>()?;
    Ok(())
}
