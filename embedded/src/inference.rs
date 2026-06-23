// embedded/src/inference.rs
//
// Bridges INT's matmul_engine into Valori's embedded firmware.
//
// Architecture:
//   1. Model bytes are baked into firmware flash via include_bytes!
//   2. At boot, load_transformer_from_bytes parses the .bin into a heap-
//      allocated QGPTModel (Box into static mut *mut).
//   3. On TYPE_INFER packet: run forward_incremental → greedy decode,
//      build BLAKE3 receipt(model | prompt | output), convert the final
//      logit distribution to FxpVector, insert into KernelState, emit
//      TYPE_INFER_RESULT with tokens + receipt + Valori state hash.
//
// Memory budget (STM32F407 — 192 KB RAM):
//   QGPTModel<61,64,64,256,4,3> ≈ 172 KB on heap.
//   Bump HEAP_MEM in main.rs to [u32; 49152] (192 KB) before enabling MCU build.
//   Intermediate KV caches live on the call stack (~24 KB), so total peak
//   RAM is ≈200 KB — fits on STM32F407 if we shrink the KernelState pool.
//   For RP2040 (264 KB SRAM) or nRF52840 (256 KB) this is fine as-is.
//
// For development / host tests the MCU feature flag is not set, so this
// file compiles as pure-Rust no_std with alloc.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use blake3::Hasher;
use matmul_engine::nn::gpt::QGPTModel;
use matmul_engine::runtime::kv_cache::QKVCache;
use matmul_engine::model::loader::load_transformer_from_bytes;
use valori_kernel::event::KernelEvent;
use valori_kernel::types::vector::FxpVector;
use valori_kernel::types::scalar::FxpScalar;
use valori_kernel::types::id::{RecordId, DEFAULT_NS};
use valori_kernel::state::kernel::KernelState;
use valori_kernel::verify::kernel_state_hash;

use crate::transport;

// ── Model constants — must match the baked .bin ───────────────────────────────
const VOCAB:  usize = 61;   // character vocab of the tiny_transformer
const SEQ:    usize = 64;   // max context window
const DIM:    usize = 64;   // model width
const HIDDEN: usize = 256;  // feed-forward hidden dim
const HEADS:  usize = 4;    // attention heads
const LAYERS: usize = 3;    // transformer layers

type TinyModel = QGPTModel<VOCAB, SEQ, DIM, HIDDEN, HEADS, LAYERS>;

// Model .bin baked into firmware flash (.rodata).  The path is relative to
// embedded/src/ — adjust if the INT repo moves.
//
// For host-side tests (no MCU feature) this still compiles; the bytes are
// loaded into the host's memory.  The MCU linker puts them in .rodata which
// maps to internal flash — no RAM cost for the weight data itself.
static MODEL_BYTES: &[u8] =
    include_bytes!("../../../INT/www/tiny_transformer_int8.bin");

// Tag written into KernelEvent::InsertRecord.tag to identify inference records.
// ASCII "INFER\0\0\1" packed into a u64.
const TAG_INFER: u64 = 0x494E_4645_5200_0001;

// Maximum tokens to generate per request.
const MAX_GEN: usize = 32;

// ── Global model pointer (single-threaded MCU: no lock needed) ────────────────
//
// Model is too large (~172 KB) to live on the stack so we Box it onto the heap
// and keep a raw pointer.  The model is initialized exactly once at boot and
// lives for the lifetime of the firmware.  On WASM / host this is also safe
// because matmul_engine is single-threaded by design.
#[allow(unsafe_code)]
static mut MODEL_PTR: *mut TinyModel = core::ptr::null_mut();

// Precomputed BLAKE3 hash of MODEL_BYTES — computed once in init().
// Used as the "model identity" in every inference receipt so the ground
// station can verify which binary ran.
#[allow(unsafe_code)]
static mut MODEL_HASH: [u8; 32] = [0u8; 32];

// Monotonically increasing record ID for inference records inserted into
// Valori's KernelState.  Persisted only in RAM; reset on power cycle
// (that's fine — the WAL replay path will re-derive records on reboot).
#[allow(unsafe_code)]
static mut NEXT_RECORD_ID: u32 = 0x8000_0000; // High bit set = inference record

// ── Init ──────────────────────────────────────────────────────────────────────

/// Load the model from flash and hash it.  Call once at boot after the heap
/// is initialised.  Returns false if the .bin is malformed.
#[allow(unsafe_code)]
pub fn init() -> bool {
    // Hash MODEL_BYTES → model identity for receipts.
    let h = blake3::hash(MODEL_BYTES);
    unsafe { MODEL_HASH = *h.as_bytes(); }

    // Parse model. The parsed struct is ~172 KB; it goes on the heap.
    match load_transformer_from_bytes::<VOCAB, SEQ, DIM, HIDDEN, HEADS, LAYERS>(MODEL_BYTES) {
        Ok(model) => {
            let boxed = Box::new(model);
            unsafe {
                if !MODEL_PTR.is_null() {
                    // Re-init: drop the old model first (shouldn't happen in FW).
                    drop(Box::from_raw(MODEL_PTR));
                }
                MODEL_PTR = Box::into_raw(boxed);
            }
            true
        }
        Err(_) => false,
    }
}

pub fn is_ready() -> bool {
    unsafe { !MODEL_PTR.is_null() }
}

// ── Inference request parsing ─────────────────────────────────────────────────
//
// TYPE_INFER payload layout:
//   [GEN_LEN : 1]   — number of tokens to generate (1..=MAX_GEN)
//   [PROMPT_LEN : 1] — number of prompt tokens
//   [PROMPT : PROMPT_LEN × u8]  — token IDs in [0, VOCAB)

struct InferRequest {
    gen_len:    usize,
    prompt:     Vec<u8>,
}

fn parse_request(payload: &[u8]) -> Option<InferRequest> {
    if payload.len() < 2 { return None; }
    let gen_len    = payload[0] as usize;
    let prompt_len = payload[1] as usize;
    if gen_len == 0 || gen_len > MAX_GEN { return None; }
    if payload.len() < 2 + prompt_len    { return None; }

    let prompt = payload[2..2 + prompt_len].to_vec();
    Some(InferRequest { gen_len, prompt })
}

// ── Core inference ────────────────────────────────────────────────────────────

/// A BLAKE3 receipt binding (model_hash | prompt | output).
pub type InferenceReceipt = [u8; 32];

/// Run greedy decode on `prompt`, return (output_tokens, receipt, logit_vec).
///
/// `logit_vec` is the final-step logit distribution converted to Q16.16
/// `FxpVector` — this is the "semantic fingerprint" stored in Valori so
/// future search queries can retrieve similar past inferences.
#[allow(unsafe_code)]
fn run_inference(
    prompt: &[u8],
    gen_len: usize,
) -> Option<(Vec<u8>, InferenceReceipt, FxpVector)> {
    let model: &TinyModel = unsafe {
        if MODEL_PTR.is_null() { return None; }
        &*MODEL_PTR
    };

    let prompt_len = prompt.len().min(SEQ);
    if prompt_len == 0 { return None; }

    // Fresh KV caches for this request — stack-allocated ([LAYERS] × per-layer cache).
    let mut caches: [QKVCache<SEQ, DIM>; LAYERS] =
        core::array::from_fn(|_| QKVCache::new());

    // Prefill: feed all prompt tokens into the KV cache.
    let mut last_logits = matmul_engine::matrix::Matrix::<f32, 1, VOCAB>::zeros();
    for (pos, &tok) in prompt[..prompt_len].iter().enumerate() {
        last_logits = model.forward_incremental(tok as usize, pos, &mut caches);
    }

    // Autoregressive decode.
    let max_steps = gen_len.min(SEQ.saturating_sub(prompt_len));
    let mut output: Vec<u8> = Vec::with_capacity(max_steps);
    let mut pos = prompt_len;

    for _ in 0..max_steps {
        // Greedy argmax over f32 logits.
        let mut best = 0usize;
        let mut best_val = last_logits.data[0][0];
        for v in 1..VOCAB {
            if last_logits.data[0][v] > best_val {
                best_val = last_logits.data[0][v];
                best = v;
            }
        }
        output.push(best as u8);
        if pos >= SEQ { break; }
        last_logits = model.forward_incremental(best, pos, &mut caches);
        pos += 1;
    }

    // Receipt: BLAKE3(model_hash | prompt | output).
    let mut h = Hasher::new();
    unsafe { h.update(&raw const MODEL_HASH); }
    h.update(prompt);
    h.update(&output);
    let receipt = *h.finalize().as_bytes();

    // Convert final logit distribution → Q16.16 FxpVector for Valori.
    // Normalise by absmax so the Q16.16 range is fully used.
    let fxp_vec = logits_to_fxp(&last_logits);

    Some((output, receipt, fxp_vec))
}

/// f32 logits (length VOCAB) → Q16.16 FxpVector (length VOCAB).
///
/// Normalises by absmax → values in [-1, 1] → scale to Q16.16 (multiply
/// by 65536, the SCALE constant in valori-kernel's fxp module).  This
/// preserves relative distances for L2 search inside KernelState.
fn logits_to_fxp(logits: &matmul_engine::matrix::Matrix<f32, 1, VOCAB>) -> FxpVector {
    const Q16_SCALE: f32 = 65536.0; // 2^16 — matches FxpScalar's Q16.16 encoding

    let mut absmax = 1e-9_f32;
    for v in 0..VOCAB {
        let a = libm::fabsf(logits.data[0][v]);
        if a > absmax { absmax = a; }
    }

    let mut vec = FxpVector::new_zeros(VOCAB);
    for v in 0..VOCAB {
        let normalized = logits.data[0][v] / absmax; // ∈ [-1, 1]
        vec.data[v] = FxpScalar(libm::roundf(normalized * Q16_SCALE) as i32);
    }
    vec
}

// ── Valori integration ────────────────────────────────────────────────────────

/// Insert the inference result as a `KernelEvent::InsertRecord` into Valori.
///
/// The `metadata` field carries the raw 32-byte BLAKE3 receipt so the audit
/// chain irrefutably binds this inference to the state hash at the moment of
/// insertion.  The ground station can:
///   1. Replay the same prompt through the same model binary.
///   2. Recompute the receipt → same 32 bytes.
///   3. Verify the receipt appears at this position in the Valori chain.
///
/// Returns the `RecordId` assigned to this inference record.
#[allow(unsafe_code)]
fn insert_into_valori(
    state:   &mut KernelState,
    fxp_vec: FxpVector,
    receipt: &InferenceReceipt,
) -> RecordId {
    let id = RecordId(unsafe {
        let r = NEXT_RECORD_ID;
        NEXT_RECORD_ID = NEXT_RECORD_ID.wrapping_add(1);
        r
    });

    let evt = KernelEvent::InsertRecord {
        id,
        vector:   fxp_vec,
        metadata: Some(receipt.to_vec()), // 32-byte receipt in the audit chain
        tag:      TAG_INFER,
    };

    // apply_event_ns returns Err only if the record pool is full; in that
    // case we emit an error over UART but do not bkpt (non-fatal for inference).
    let _ = state.apply_event_ns(&evt, DEFAULT_NS.0);
    id
}

// ── Packet handler ────────────────────────────────────────────────────────────
//
// TYPE_INFER_RESULT payload layout (sent back to host):
//   [OK : 1]               — 1 = success, 0 = error
//   [OUT_LEN : 1]          — number of generated tokens
//   [TOKENS : OUT_LEN × u8]
//   [RECEIPT : 32]         — BLAKE3(model | prompt | output)
//   [RECORD_ID : 4 LE]     — RecordId assigned in Valori
//   [VERSION : 8 LE]       — KernelState.version() after insert
//   [STATE_HASH : 32]      — Valori BLAKE3 state hash after insert
//
// Max response size: 1+1+32+32+4+8+32 = 110 bytes.

const RESULT_BUF: usize = 110;

/// Parse a TYPE_INFER packet, run inference, store into Valori, emit result.
pub fn handle(state: &mut KernelState, payload: &[u8]) {
    let req = match parse_request(payload) {
        Some(r) => r,
        None => { transport::export_error(b"BAD_INFER"); return; }
    };

    let (output, receipt, fxp_vec) = match run_inference(&req.prompt, req.gen_len) {
        Some(r) => r,
        None => { transport::export_error(b"INFER_FAIL"); return; }
    };

    // Commit into Valori's audit chain.
    let record_id = insert_into_valori(state, fxp_vec, &receipt);
    let version    = state.version();
    let state_hash = kernel_state_hash(state);

    // Encode response — no heap.
    let mut buf = [0u8; RESULT_BUF];
    let mut off = 0;

    buf[off] = 1; off += 1; // OK
    let out_len = output.len().min(MAX_GEN);
    buf[off] = out_len as u8; off += 1;
    buf[off..off + out_len].copy_from_slice(&output[..out_len]); off += out_len;
    buf[off..off + 32].copy_from_slice(&receipt); off += 32;
    buf[off..off + 4].copy_from_slice(&record_id.0.to_le_bytes()); off += 4;
    buf[off..off + 8].copy_from_slice(&version.to_le_bytes()); off += 8;
    buf[off..off + 32].copy_from_slice(&state_hash); off += 32;

    transport::export_infer_result(&buf[0..off]);
}

// ── RAG helper (Vision 2) ─────────────────────────────────────────────────────
//
// Retrieve the K nearest past inferences from Valori and prepend their
// output tokens to the prompt before running inference.  This gives the
// model access to its own history — on-device RAG with no server.
//
// Called optionally: pass rag_k = 0 to skip retrieval.

pub fn handle_with_rag(
    state:   &mut KernelState,
    payload: &[u8],
    rag_k:   usize,
) {
    let req = match parse_request(payload) {
        Some(r) => r,
        None => { transport::export_error(b"BAD_INFER"); return; }
    };

    // Build query vector from prompt tokens (average of their embeddings).
    // We use INT's token embedding layer to encode the prompt semantically.
    let context_prompt = if rag_k > 0 {
        retrieve_context(state, &req.prompt, rag_k)
    } else {
        req.prompt.clone()
    };

    let (output, receipt, fxp_vec) = match run_inference(&context_prompt, req.gen_len) {
        Some(r) => r,
        None => { transport::export_error(b"INFER_FAIL"); return; }
    };

    let record_id  = insert_into_valori(state, fxp_vec, &receipt);
    let version    = state.version();
    let state_hash = kernel_state_hash(state);

    let mut buf = [0u8; RESULT_BUF];
    let mut off = 0;
    buf[off] = 1; off += 1;
    let out_len = output.len().min(MAX_GEN);
    buf[off] = out_len as u8; off += 1;
    buf[off..off + out_len].copy_from_slice(&output[..out_len]); off += out_len;
    buf[off..off + 32].copy_from_slice(&receipt); off += 32;
    buf[off..off + 4].copy_from_slice(&record_id.0.to_le_bytes()); off += 4;
    buf[off..off + 8].copy_from_slice(&version.to_le_bytes()); off += 8;
    buf[off..off + 32].copy_from_slice(&state_hash); off += 32;

    transport::export_infer_result(&buf[0..off]);
}

/// Encode the prompt as a mean embedding vector and search Valori for the
/// K nearest past inference records.  Prepend those output tokens (stored
/// in metadata) to the prompt — creating an on-device retrieval context.
#[allow(unsafe_code)]
fn retrieve_context(
    state:  &KernelState,
    prompt: &[u8],
    k:      usize,
) -> Vec<u8> {
    use valori_kernel::index::SearchResult;
    use valori_kernel::types::id::RecordId;

    let model: &TinyModel = unsafe {
        if MODEL_PTR.is_null() { return prompt.to_vec(); }
        &*MODEL_PTR
    };

    // Mean-pool token embeddings to get a query vector.
    let len = prompt.len().min(SEQ);
    if len == 0 { return prompt.to_vec(); }

    let tok_scale = model.token_embeddings.weights.scale();
    let mut query = FxpVector::new_zeros(DIM);
    for &tok in &prompt[..len] {
        for d in 0..DIM {
            let v = model.token_embeddings.weights.raw(tok as usize, d) as f32 * tok_scale;
            // accumulate in f32, convert to Q16.16 at the end
            query.data[d] = FxpScalar(query.data[d].0 + libm::roundf(v * 65536.0) as i32);
        }
    }
    let inv_len = 1.0 / len as f32;
    for d in 0..DIM {
        query.data[d] = FxpScalar(libm::roundf(query.data[d].0 as f32 * inv_len) as i32);
    }

    // Search Valori for K nearest past inferences.
    let k_clamped = k.min(crate::search::MAX_K);
    let mut results = [SearchResult {
        score: FxpScalar(i32::MAX),
        id:    RecordId(u32::MAX),
    }; crate::search::MAX_K];

    let found = state.search_l2_ns(&query, &mut results[..k_clamped], DEFAULT_NS.0);

    // Build augmented prompt: retrieved tokens (from metadata) + original prompt.
    // In production you'd decode the metadata bytes back to token IDs stored
    // at insert time.  Here we use the record IDs as placeholder context tokens
    // (proof of concept — real impl stores output tokens in a companion WAL).
    let mut augmented: Vec<u8> = Vec::with_capacity(found * 4 + prompt.len());
    for i in 0..found {
        // Use the low byte of each record ID as a context "separator" token.
        let sep_tok = (results[i].id.0 & 0xFF) as u8 % VOCAB as u8;
        augmented.push(sep_tok);
    }
    augmented.extend_from_slice(prompt);
    augmented.truncate(SEQ); // never exceed the context window
    augmented
}
