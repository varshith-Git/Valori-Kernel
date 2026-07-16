// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Effect — the unit of side-effectful work dispatched by Tasks through the EffectBus.
//!
//! All kernel writes, receipt fragments, and audit entries flow as Effects. The bus
//! deduplicates by `EffectId` so retried tasks cannot double-write.
use serde::{Deserialize, Serialize};
use valori_core::id::ExecutionId;

// ── EffectId ──────────────────────────────────────────────────────────────────

/// Unique, stable ID for one Effect emission.
///
/// Constructed as `BLAKE3(execution_id ‖ task_index ‖ effect_index_within_task)`.
/// The same task re-run on retry produces the same `EffectId`s, enabling dedup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EffectId(pub [u8; 32]);

impl EffectId {
    pub fn new(execution_id: &ExecutionId, task_index: u32, effect_index: u32) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&execution_id.hi.to_le_bytes());
        hasher.update(&execution_id.lo.to_le_bytes());
        hasher.update(&task_index.to_le_bytes());
        hasher.update(&effect_index.to_le_bytes());
        EffectId(*hasher.finalize().as_bytes())
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// ── EffectDurability ──────────────────────────────────────────────────────────

/// Whether an Effect must be persisted before the task completes.
///
/// `Durable` effects (kernel writes, receipt fragments, audit entries) are awaited
/// before `task.run()` returns. `Ephemeral` effects (metrics, traces) are
/// fire-and-forget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectDurability {
    /// Must complete before the task is marked Done.
    Durable,
    /// Best-effort; task does not wait for completion.
    Ephemeral,
}

// ── KernelCommandBody ─────────────────────────────────────────────────────────

/// The structured payload of a kernel mutation command.
///
/// Capabilities pattern-match on this to dispatch to the appropriate kernel
/// method. Kept in `valori-effect` so neither side depends on the other's
/// concrete types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum KernelCommandBody {
    InsertRecord {
        values: Vec<f32>,
        text: Option<String>,
        metadata: Option<serde_json::Value>,
        tag: u8,
    },
    SoftDeleteRecord {
        record_id: u32,
    },
    HardDeleteRecord {
        record_id: u32,
    },
    CreateNode {
        kind: u8,
        record_id: Option<u32>,
    },
    CreateEdge {
        from: u32,
        to: u32,
        kind: u8,
    },
}

// ── KernelCommand ─────────────────────────────────────────────────────────────

/// A kernel mutation command routed through the EffectBus.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelCommand {
    pub shard_id: u8,
    pub namespace_id: u16,
    pub body: KernelCommandBody,
    /// Idempotency key (request_id for Raft dedup).
    pub request_id: String,
}

// ── ReceiptFragment ───────────────────────────────────────────────────────────

/// A fragment of a proof Receipt, emitted by each task that modifies state.
///
/// The `ReceiptAssembler` (Phase A8) collects these in topological order and
/// chains them into the final `Receipt`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReceiptFragment {
    pub task_index: u32,
    /// BLAKE3 hex of the kernel state before this task.
    pub state_hash_before: String,
    /// BLAKE3 hex of the kernel state after this task. Equal to `before` for reads.
    pub state_hash_after: String,
    /// True if this task produced kernel writes.
    pub mutated: bool,
    /// BLAKE3 hex of the fragment itself (for chaining).
    pub fragment_hash: String,
}

impl ReceiptFragment {
    pub fn read_only(task_index: u32, state_hash: String) -> Self {
        let fragment_hash = {
            let mut h = blake3::Hasher::new();
            h.update(state_hash.as_bytes());
            h.update(&task_index.to_le_bytes());
            h.finalize().to_hex().to_string()
        };
        ReceiptFragment {
            task_index,
            state_hash_before: state_hash.clone(),
            state_hash_after: state_hash,
            mutated: false,
            fragment_hash,
        }
    }
}

// ── Effect ────────────────────────────────────────────────────────────────────

/// A side-effectful work unit emitted by a Task and routed by the EffectBus.
///
/// Every kernel write, receipt fragment, metric, and audit entry flows as an
/// `Effect`. The bus deduplicates Durable effects by `EffectId`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub id: EffectId,
    pub durability: EffectDurability,
    pub payload: EffectPayload,
}

impl Effect {
    pub fn durable(id: EffectId, payload: EffectPayload) -> Self {
        Effect {
            id,
            durability: EffectDurability::Durable,
            payload,
        }
    }

    pub fn ephemeral(id: EffectId, payload: EffectPayload) -> Self {
        Effect {
            id,
            durability: EffectDurability::Ephemeral,
            payload,
        }
    }
}

/// The concrete payload of an Effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectPayload {
    /// Apply a mutation to the kernel (Durable).
    KernelWrite(KernelCommand),
    /// Emit a receipt fragment for proof assembly (Durable).
    Receipt(ReceiptFragment),
    /// Increment a named counter metric (Ephemeral).
    Counter { name: String, value: f64 },
    /// Record a named gauge metric (Ephemeral).
    Gauge { name: String, value: f64 },
    /// Emit an audit log entry (Durable).
    Audit { entry_json: String, shard_id: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_id_is_stable() {
        let eid = ExecutionId { hi: 1, lo: 2 };
        let id1 = EffectId::new(&eid, 0, 0);
        let id2 = EffectId::new(&eid, 0, 0);
        assert_eq!(id1, id2);

        let id3 = EffectId::new(&eid, 0, 1);
        assert_ne!(id1, id3);
    }

    #[test]
    fn receipt_fragment_read_only() {
        let frag = ReceiptFragment::read_only(2, "abc123".into());
        assert!(!frag.mutated);
        assert_eq!(frag.state_hash_before, frag.state_hash_after);
        assert_eq!(frag.task_index, 2);
    }

    #[test]
    fn effect_roundtrip_json() {
        let eid = ExecutionId { hi: 0, lo: 1 };
        let e = Effect::durable(
            EffectId::new(&eid, 0, 0),
            EffectPayload::Counter {
                name: "test".into(),
                value: 1.0,
            },
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: Effect = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, e.id);
    }
}
