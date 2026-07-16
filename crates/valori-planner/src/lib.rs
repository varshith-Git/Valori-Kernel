// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-planner` — Operation lifecycle and execution planning.
//!
//! Turns an [`Operation`] + [`PlanningContext`] into a deterministic
//! [`ExecutionGraph`] DAG of [`TaskSpec`]s. Caches graphs at two layers:
//! in-process (`ExecutionCache`) and durable (`MetadataDb`).
//!
//! # Crate boundary
//! This crate is std-only. It must NOT be added as a dependency of
//! `valori-kernel` or `valori-core`.

pub mod context;
pub mod error;
pub mod graph;
pub mod operation;
pub mod planner;
pub mod registry;

// Top-level re-exports for the most commonly used types.
pub use context::{CapabilitySet, PlannerFingerprint, PlanningContext, PlanningContextHash};
pub use error::{PlannerError, PlannerResult};
pub use graph::{
    ExecutionGraph, ExecutionRetentionPolicy, GraphHash, TaskEdge, TaskId, TaskKind, TaskSpec,
};
pub use operation::{
    compute_operation_hash, ConsistencyLevel, ExecutionPolicy, Operation, OperationHash,
    OperationId, OperationInputs, OperationKind, ResourceBudget,
};
pub use planner::{plan_with_cache, IngestPlanner, NoOpPlanner, Planner};
pub use registry::{
    CacheKey, ExecutionCache, ExecutionContext, ExecutionHandle, ExecutionRegistry,
    ExecutionStatus, TaskState,
};
