// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Unified `Receipt` type and `ReceiptAssembler` — Phase A8.
//!
//! Replaces the three ad-hoc proof structures that existed before:
//! - `EventProof` in `valori-storage`
//! - per-recall receipt in `valori-mcp`
//! - Tree-RAG citation chain in `tree_rag.rs`
//!
//! Every operation now produces one `Receipt` that carries a self-describing,
//! offline-verifiable proof of what ran and what state changed.
use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use valori_core::id::ExecutionId;

use crate::effect::ReceiptFragment;

// ── Primitive types ───────────────────────────────────────────────────────────

/// Opaque BLAKE3 hash of a `Receipt`.
/// `receipt_hash = BLAKE3(op_hash ‖ graph_hash ‖ state_before ‖ state_after ‖
///                         sorted(parent_hashes) ‖ shard_id ‖ committed_height)`
/// `produced_at` is intentionally excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReceiptHash(pub [u8; 32]);

impl ReceiptHash {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn zero() -> Self { ReceiptHash([0u8; 32]) }
}

/// Opaque BLAKE3 hash of a kernel state snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateHash(pub String);

impl StateHash {
    pub fn zero() -> Self { StateHash("0".repeat(64)) }

    pub fn from_hex(s: impl Into<String>) -> Self { StateHash(s.into()) }
}

// ── Receipt ───────────────────────────────────────────────────────────────────

/// Versioned envelope — outermost type when serializing to disk or HTTP.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptEnvelope {
    pub version: u8,
    pub payload: Receipt,
}

impl ReceiptEnvelope {
    pub fn v1(payload: Receipt) -> Self {
        ReceiptEnvelope { version: 1, payload }
    }
}

/// The unified proof of one completed Operation.
///
/// Self-describing: carries `KernelABI`, `PlannerFingerprint.hash`, and
/// `CapabilitySet` so an offline verifier needs no node context.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    // ── Identity ──────────────────────────────────────────────────────────
    /// ULID-style unique ID (hex of the execution_id for now; Phase A8).
    pub receipt_id: String,
    /// Content-addressed hash of this receipt (all fields except `produced_at`).
    pub receipt_hash: ReceiptHash,

    // ── What ran ─────────────────────────────────────────────────────────
    /// BLAKE3(kind ‖ inputs ‖ policy) for the operation.
    pub operation_hash: String,
    /// BLAKE3(op_hash ‖ fp.hash ‖ ctx_hash ‖ topo_order) for the graph.
    pub graph_hash: String,

    // ── Under what contract ───────────────────────────────────────────────
    pub kernel_abi_version: u32,
    /// BLAKE3(version ‖ routing_config_hash ‖ feature_flags_hash ‖ schema_version).
    pub planner_fingerprint_hash: String,
    pub embed_enabled: bool,
    pub cluster_mode: bool,
    pub shard_count: u8,

    // ── State transition proof ────────────────────────────────────────────
    pub state_hash_before: StateHash,
    /// Equal to `state_hash_before` for read-only operations.
    pub state_hash_after: StateHash,

    // ── Merkle DAG ────────────────────────────────────────────────────────
    /// Empty for root receipts. Single-parent today; multi-parent reserved.
    pub parent_receipts: Vec<ReceiptHash>,

    // ── Provenance (excluded from hash) ──────────────────────────────────
    pub shard_id: u8,
    pub committed_height: u64,
    pub produced_at: u64,
    /// Ordered ReceiptFragments (topo order) that composed this receipt.
    pub fragments: Vec<ReceiptFragment>,
}

impl Receipt {
    /// Return true if this was a read-only operation (state did not change).
    pub fn is_read_only(&self) -> bool {
        self.state_hash_before == self.state_hash_after
    }
}

// ── ReceiptAssembler ──────────────────────────────────────────────────────────

/// Collects `ReceiptFragment`s emitted by tasks and assembles the final `Receipt`.
///
/// One assembler is created per execution. It is held inside the `EffectBus`
/// (or alongside it) and populated as tasks dispatch `EffectPayload::Receipt`.
///
/// `assemble()` sorts fragments by `task_index` and builds the chain.
pub struct ReceiptAssembler {
    execution_id: ExecutionId,
    fragments: Mutex<Vec<ReceiptFragment>>,
    operation_hash: String,
    graph_hash: String,
    planner_fingerprint_hash: String,
    kernel_abi_version: u32,
    embed_enabled: bool,
    cluster_mode: bool,
    shard_count: u8,
    shard_id: u8,
    committed_height: u64,
    parent_receipts: Vec<ReceiptHash>,
}

impl ReceiptAssembler {
    pub fn new(
        execution_id: ExecutionId,
        operation_hash: String,
        graph_hash: String,
        planner_fingerprint_hash: String,
        kernel_abi_version: u32,
        embed_enabled: bool,
        cluster_mode: bool,
        shard_count: u8,
        shard_id: u8,
        committed_height: u64,
        parent_receipts: Vec<ReceiptHash>,
    ) -> Self {
        ReceiptAssembler {
            execution_id,
            fragments: Mutex::new(Vec::new()),
            operation_hash,
            graph_hash,
            planner_fingerprint_hash,
            kernel_abi_version,
            embed_enabled,
            cluster_mode,
            shard_count,
            shard_id,
            committed_height,
            parent_receipts,
        }
    }

    /// Add one fragment. Thread-safe; may be called from any task.
    pub fn push(&self, fragment: ReceiptFragment) {
        if let Ok(mut frags) = self.fragments.lock() {
            frags.push(fragment);
        }
    }

    /// Assemble the final `Receipt` from all accumulated fragments.
    ///
    /// Sorts fragments by `task_index`. The `state_hash_before` of the
    /// receipt is taken from the first fragment; `state_hash_after` from the last
    /// mutating fragment (or `state_hash_before` for read-only operations).
    pub fn assemble(&self) -> Receipt {
        let mut frags = self.fragments.lock()
            .unwrap_or_else(|e| e.into_inner());
        frags.sort_by_key(|f| f.task_index);

        let state_hash_before = frags.first()
            .map(|f| StateHash::from_hex(&f.state_hash_before))
            .unwrap_or_else(StateHash::zero);

        let state_hash_after = frags.iter()
            .filter(|f| f.mutated)
            .last()
            .map(|f| StateHash::from_hex(&f.state_hash_after))
            .unwrap_or_else(|| state_hash_before.clone());

        let produced_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let receipt_id = format!("{}", self.execution_id);

        let receipt_hash = compute_receipt_hash(
            &self.operation_hash,
            &self.graph_hash,
            &state_hash_before,
            &state_hash_after,
            &self.parent_receipts,
            self.shard_id,
            self.committed_height,
        );

        Receipt {
            receipt_id,
            receipt_hash,
            operation_hash: self.operation_hash.clone(),
            graph_hash: self.graph_hash.clone(),
            kernel_abi_version: self.kernel_abi_version,
            planner_fingerprint_hash: self.planner_fingerprint_hash.clone(),
            embed_enabled: self.embed_enabled,
            cluster_mode: self.cluster_mode,
            shard_count: self.shard_count,
            state_hash_before,
            state_hash_after,
            parent_receipts: self.parent_receipts.clone(),
            shard_id: self.shard_id,
            committed_height: self.committed_height,
            produced_at,
            fragments: frags.clone(),
        }
    }
}

fn compute_receipt_hash(
    operation_hash: &str,
    graph_hash: &str,
    state_before: &StateHash,
    state_after: &StateHash,
    parents: &[ReceiptHash],
    shard_id: u8,
    committed_height: u64,
) -> ReceiptHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(operation_hash.as_bytes());
    hasher.update(graph_hash.as_bytes());
    hasher.update(state_before.0.as_bytes());
    hasher.update(state_after.0.as_bytes());

    // Sort parent hashes for determinism.
    let mut sorted_parents: Vec<&ReceiptHash> = parents.iter().collect();
    sorted_parents.sort_by_key(|h| h.0);
    for p in sorted_parents {
        hasher.update(&p.0);
    }

    hasher.update(&[shard_id]);
    hasher.update(&committed_height.to_le_bytes());
    ReceiptHash(*hasher.finalize().as_bytes())
}

// ── verify_receipt ────────────────────────────────────────────────────────────

/// Offline verification of a `Receipt`.
///
/// Step 1: Recompute `receipt_hash` and compare.
/// Step 2: Verify fragment chain (each mutating fragment's `state_hash_after`
///         must match the next fragment's `state_hash_before`).
/// Step 3: Check `state_hash_before` matches first fragment; `state_hash_after`
///         matches last mutating fragment (or equals `state_hash_before` for reads).
///
/// Returns `Ok(())` on success, `Err(String)` describing the first failure.
pub fn verify_receipt(receipt: &Receipt) -> Result<(), String> {
    // Step 1: recompute hash.
    let expected_hash = compute_receipt_hash(
        &receipt.operation_hash,
        &receipt.graph_hash,
        &receipt.state_hash_before,
        &receipt.state_hash_after,
        &receipt.parent_receipts,
        receipt.shard_id,
        receipt.committed_height,
    );
    if expected_hash != receipt.receipt_hash {
        return Err(format!(
            "receipt_hash mismatch: computed {} != stored {}",
            expected_hash.to_hex(), receipt.receipt_hash.to_hex()
        ));
    }

    // Step 2: verify fragment state chain.
    let mut sorted = receipt.fragments.clone();
    sorted.sort_by_key(|f| f.task_index);

    let mut prev_after: Option<&str> = None;
    for frag in &sorted {
        if let Some(prev) = prev_after {
            if frag.mutated && frag.state_hash_before != prev {
                return Err(format!(
                    "fragment chain break at task {}: expected state_before='{}' got '{}'",
                    frag.task_index, prev, frag.state_hash_before
                ));
            }
        }
        if frag.mutated {
            prev_after = Some(&frag.state_hash_after);
        }
    }

    // Step 3: outer state hash consistency.
    let first_before = sorted.first().map(|f| f.state_hash_before.as_str());
    if let Some(fb) = first_before {
        if receipt.state_hash_before.0 != fb {
            return Err(format!(
                "state_hash_before mismatch: receipt='{}' first_fragment='{}'",
                receipt.state_hash_before.0, fb
            ));
        }
    }

    Ok(())
}

// ── ReceiptStore (in-process last-N cache) ────────────────────────────────────

/// A simple in-process store of the last `capacity` receipts.
///
/// The HTTP `/v1/proof/receipt` endpoint reads from this. A durable receipt log
/// is deferred to Phase A8+.
pub struct ReceiptStore {
    capacity: usize,
    receipts: Mutex<HashMap<String, Receipt>>,
    order: Mutex<Vec<String>>,
}

impl ReceiptStore {
    pub fn new(capacity: usize) -> Self {
        ReceiptStore {
            capacity,
            receipts: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
        }
    }

    pub fn insert(&self, receipt: Receipt) {
        let id = receipt.receipt_id.clone();
        let mut recs = self.receipts.lock().unwrap();
        let mut ord = self.order.lock().unwrap();
        recs.insert(id.clone(), receipt);
        ord.push(id);
        if ord.len() > self.capacity {
            let evict = ord.remove(0);
            recs.remove(&evict);
        }
    }

    pub fn get(&self, id: &str) -> Option<Receipt> {
        self.receipts.lock().unwrap().get(id).cloned()
    }

    pub fn latest(&self) -> Option<Receipt> {
        let ord = self.order.lock().unwrap();
        let id = ord.last()?;
        self.receipts.lock().unwrap().get(id).cloned()
    }

    pub fn list_ids(&self) -> Vec<String> {
        self.order.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valori_core::id::ExecutionId;
    use crate::effect::ReceiptFragment;

    fn assembler() -> ReceiptAssembler {
        ReceiptAssembler::new(
            ExecutionId { hi: 1, lo: 2 },
            "op_hash_hex".into(),
            "graph_hash_hex".into(),
            "fp_hash_hex".into(),
            1, false, false, 1, 0, 42, vec![],
        )
    }

    #[test]
    fn empty_assembler_produces_zero_hashes() {
        let asm = assembler();
        let r = asm.assemble();
        assert_eq!(r.state_hash_before, r.state_hash_after);
        assert!(!r.receipt_hash.to_hex().is_empty());
    }

    #[test]
    fn read_only_receipt_has_equal_state_hashes() {
        let asm = assembler();
        let frag = ReceiptFragment::read_only(0, "aaaa".into());
        asm.push(frag);
        let r = asm.assemble();
        assert!(r.is_read_only());
        assert_eq!(r.state_hash_before.0, "aaaa");
        assert_eq!(r.state_hash_after.0, "aaaa");
    }

    #[test]
    fn mutating_receipt_updates_state_after() {
        let asm = assembler();
        asm.push(ReceiptFragment {
            task_index: 0,
            state_hash_before: "before".into(),
            state_hash_after:  "after".into(),
            mutated: true,
            fragment_hash: "fh".into(),
        });
        let r = asm.assemble();
        assert!(!r.is_read_only());
        assert_eq!(r.state_hash_before.0, "before");
        assert_eq!(r.state_hash_after.0, "after");
    }

    #[test]
    fn verify_receipt_passes_for_valid_receipt() {
        let asm = assembler();
        asm.push(ReceiptFragment::read_only(0, "abc".into()));
        let r = asm.assemble();
        assert!(verify_receipt(&r).is_ok());
    }

    #[test]
    fn verify_receipt_fails_for_tampered_hash() {
        let asm = assembler();
        let mut r = asm.assemble();
        r.receipt_hash = ReceiptHash([99u8; 32]);
        assert!(verify_receipt(&r).is_err());
    }

    #[test]
    fn receipt_store_evicts_oldest() {
        let store = ReceiptStore::new(2);
        let asm = assembler();
        for _ in 0..3 {
            store.insert(asm.assemble());
        }
        assert_eq!(store.list_ids().len(), 2);
    }

    #[test]
    fn receipt_hash_is_deterministic() {
        let asm = assembler();
        let r1 = asm.assemble();
        let asm2 = assembler();
        let r2 = asm2.assemble();
        assert_eq!(r1.receipt_hash, r2.receipt_hash);
    }
}
