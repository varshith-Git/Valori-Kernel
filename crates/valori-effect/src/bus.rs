// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! EffectBus — the single routing layer between Tasks and subsystems.
//!
//! All side effects flow through here. The bus:
//! - Deduplicates Durable effects by `EffectId` (safe for retried tasks).
//! - Dispatches `KernelWrite` to `KernelCapability::apply_command`.
//! - Dispatches `Receipt` fragments to `ProofCapability::append_fragment`.
//! - Dispatches `Audit` entries to `ProofCapability`.
//! - Fire-and-forgets `Ephemeral` effects (metrics).
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::capability::CapabilityRegistry;
use crate::effect::{Effect, EffectDurability, EffectId, EffectPayload};
use crate::error::{EffectError, EffectResult};

// ── EffectBus ─────────────────────────────────────────────────────────────────

/// Routes Effects from Tasks to capabilities.
///
/// One `EffectBus` is created per execution and shared across all tasks in that
/// execution. The dedup set is scoped to the execution — retrying the same task
/// in a different execution uses a fresh bus (and fresh `EffectId`s generated
/// from the new `ExecutionId`).
pub struct EffectBus {
    caps: Arc<CapabilityRegistry>,
    dispatched: Mutex<HashSet<EffectId>>,
}

impl EffectBus {
    pub fn new(caps: Arc<CapabilityRegistry>) -> Self {
        EffectBus {
            caps,
            dispatched: Mutex::new(HashSet::new()),
        }
    }

    /// Dispatch one effect.
    ///
    /// - **Durable**: checked against the dedup set first. If already dispatched,
    ///   returns `Err(EffectError::Duplicate)` — the caller should treat this as a
    ///   no-op, not a failure (indicates a retry saw an already-committed effect).
    /// - **Ephemeral**: fire-and-forget; dedup set not consulted.
    /// Dispatch one effect. Returns the capability's result value for
    /// `KernelWrite` effects (e.g. `{"record_id": N, "state_hash": "..."}`),
    /// or `Value::Null` for all other effect kinds.
    pub async fn dispatch(&self, effect: Effect) -> EffectResult<serde_json::Value> {
        if effect.durability == EffectDurability::Durable {
            let already = {
                let mut set = self.dispatched.lock().await;
                !set.insert(effect.id)
            };
            if already {
                debug!(effect_id = %effect.id.to_hex(), "EffectBus: dedup skip");
                return Err(EffectError::Duplicate(effect.id.to_hex()));
            }
        }
        self.route(effect).await
    }

    async fn route(&self, effect: Effect) -> EffectResult<serde_json::Value> {
        match effect.payload {
            EffectPayload::KernelWrite(cmd) => {
                let result = self
                    .caps
                    .kernel
                    .apply_command(cmd.shard_id, cmd.namespace_id, &cmd.body, &cmd.request_id)
                    .await?;
                debug!("EffectBus: KernelWrite dispatched (shard={})", cmd.shard_id);
                Ok(result)
            }
            EffectPayload::Receipt(frag) => {
                if let Ok(proof) = self.caps.proof() {
                    let json = serde_json::to_string(&frag)
                        .map_err(|e| EffectError::Dispatch(e.to_string()))?;
                    proof.append_fragment(0, &json).await?;
                } else {
                    debug!("EffectBus: ProofCapability absent — receipt fragment dropped");
                }
                Ok(serde_json::Value::Null)
            }
            EffectPayload::Audit {
                entry_json,
                shard_id,
            } => {
                if let Ok(proof) = self.caps.proof() {
                    proof.append_fragment(shard_id, &entry_json).await?;
                } else {
                    debug!("EffectBus: ProofCapability absent — audit entry dropped");
                }
                Ok(serde_json::Value::Null)
            }
            EffectPayload::Counter { name, value } => {
                debug!(%name, %value, "EffectBus: counter (ephemeral)");
                Ok(serde_json::Value::Null)
            }
            EffectPayload::Gauge { name, value } => {
                debug!(%name, %value, "EffectBus: gauge (ephemeral)");
                Ok(serde_json::Value::Null)
            }
        }
    }

    /// Dispatch a batch of effects, stopping on the first Durable failure that
    /// is not a dedup error. Dedup errors are silently skipped.
    pub async fn dispatch_all(&self, effects: Vec<Effect>) -> EffectResult<()> {
        for effect in effects {
            let is_durable = effect.durability == EffectDurability::Durable;
            match self.dispatch(effect).await {
                Ok(_) => {}
                Err(EffectError::Duplicate(_)) => {} // idempotent retry — skip
                Err(e) if is_durable => return Err(e),
                Err(e) => warn!("EffectBus: ephemeral effect error (ignored): {}", e),
            }
        }
        Ok(())
    }

    /// How many distinct Durable effects have been dispatched so far.
    pub async fn durable_count(&self) -> usize {
        self.dispatched.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{CapabilityRegistry, NoOpKernelCapability};
    use crate::effect::{Effect, EffectId, EffectPayload};
    use valori_core::id::ExecutionId;

    fn bus() -> EffectBus {
        EffectBus::new(Arc::new(CapabilityRegistry {
            kernel: Arc::new(NoOpKernelCapability { shard_count: 1 }),
            embed: None,
            llm: None,
            storage: None,
            http: None,
            proof: None,
            scheduler: None,
        }))
    }

    fn eid() -> ExecutionId {
        ExecutionId { hi: 42, lo: 7 }
    }

    #[tokio::test]
    async fn counter_dispatches_ok() {
        let bus = bus();
        let id = EffectId::new(&eid(), 0, 0);
        let e = Effect::ephemeral(
            id,
            EffectPayload::Counter {
                name: "test".into(),
                value: 1.0,
            },
        );
        bus.dispatch(e).await.unwrap();
    }

    #[tokio::test]
    async fn durable_dedup_returns_error() {
        use crate::effect::KernelCommandBody;
        let bus = bus();
        let id = EffectId::new(&eid(), 0, 0);
        let kernel_cmd = crate::effect::KernelCommand {
            shard_id: 0,
            namespace_id: 0,
            body: KernelCommandBody::InsertRecord {
                values: vec![],
                text: None,
                metadata: None,
                tag: 0,
            },
            request_id: "req-1".into(),
        };
        let e1 = Effect::durable(id, EffectPayload::KernelWrite(kernel_cmd.clone()));
        let e2 = Effect::durable(id, EffectPayload::KernelWrite(kernel_cmd));

        let _ = bus.dispatch(e1).await.unwrap();
        let result = bus.dispatch(e2).await;
        assert!(matches!(result, Err(EffectError::Duplicate(_))));
    }

    #[tokio::test]
    async fn dispatch_all_skips_dedup_errors() {
        use crate::effect::KernelCommandBody;
        let bus = bus();
        let id = EffectId::new(&eid(), 0, 0);
        let kernel_cmd = crate::effect::KernelCommand {
            shard_id: 0,
            namespace_id: 0,
            body: KernelCommandBody::InsertRecord {
                values: vec![],
                text: None,
                metadata: None,
                tag: 0,
            },
            request_id: "req-2".into(),
        };
        let effects = vec![
            Effect::durable(id, EffectPayload::KernelWrite(kernel_cmd.clone())),
            Effect::durable(id, EffectPayload::KernelWrite(kernel_cmd)), // duplicate
        ];
        bus.dispatch_all(effects).await.unwrap();
        assert_eq!(bus.durable_count().await, 1);
    }
}
