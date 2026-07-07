// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-effect` — Effect system and EffectBus for Valori.
//!
//! This crate is **std-only**. Do not add it as a dependency of `valori-kernel`
//! or `valori-core`.
//!
//! ## Architecture
//!
//! ```text
//! Task
//!  └─ ctx.bus.dispatch(Effect) ──→ EffectBus
//!       ├─ dedup by EffectId
//!       ├─ KernelWrite ──────────→ KernelCapability::apply_command
//!       ├─ Receipt ──────────────→ ProofCapability::append_fragment
//!       ├─ Audit ────────────────→ ProofCapability::append_fragment
//!       └─ Counter/Gauge ────────→ (log only — Phase A7 wires real metrics)
//! ```

pub mod bus;
pub mod capability;
pub mod effect;
pub mod error;
pub mod receipt;
pub mod task;
pub mod tasks;

// Top-level re-exports.
pub use bus::EffectBus;
pub use capability::{
    Capability, CapabilityRegistry, EmbedCapability, HttpCapability, KernelCapability,
    LlmCapability, NoOpKernelCapability, ProofCapability, SchedulerCapability, StorageCapability,
};
pub use effect::{Effect, EffectDurability, EffectId, EffectPayload, KernelCommand, ReceiptFragment};
pub use error::{EffectError, EffectResult};
pub use receipt::{Receipt, ReceiptAssembler, ReceiptEnvelope, ReceiptHash, ReceiptStore, StateHash, verify_receipt};
pub use task::{NoOpTask, Task, TaskContext, TaskOutput};
