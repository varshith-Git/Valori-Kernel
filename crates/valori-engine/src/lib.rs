// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Stateful engine orchestrator for the Valori platform.
//!
//! This crate houses the [`Engine`] struct and all its supporting types,
//! extracted from `valori-node` so that the engine layer can be used and
//! tested independently of the HTTP routing layer.
//!
//! ## Modules
//!
//! | Module | Contents |
//! |---|---|
//! | `config`      | [`IndexKind`], [`QuantizationKind`], [`EngineConfig`] |
//! | `error`       | [`EngineError`], [`CommitError`] |
//! | `metadata`    | [`MetadataStore`] — in-process JSON key-value sidecar |
//! | `persistence` | [`Persistence`] — standalone durability funnel |
//! | `engine`      | [`Engine`] struct + all orchestration impl blocks |

pub mod config;
pub mod engine;
pub mod error;
pub mod metadata;
pub mod persistence;

pub use config::{EngineConfig, IndexKind, QuantizationKind};
pub use engine::{Engine, EngineHealth, ExecutionResources, PoolStats, RecoveryMode};
pub use error::{CommitError, EngineError};
pub use metadata::MetadataStore;
pub use persistence::Persistence;
