// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Errors produced by this crate.
//!
//! Parsing is the only fallible operation in `valori-domain`. Nothing here
//! panics, allocates unboundedly, or performs I/O.

use thiserror::Error;

/// Result alias for domain operations.
pub type Result<T> = core::result::Result<T, DomainError>;

/// Everything that can go wrong turning untrusted input into a domain type.
///
/// Every variant carries enough context to be surfaced directly to an API
/// caller: the kind of identifier and what was wrong with it. None of them
/// carry the raw input verbatim beyond what is needed to diagnose, so these
/// messages are safe to log.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// A `ProjectId`, `SessionId` or `InstallationId` was not a valid UUID.
    #[error("invalid {kind}: not a valid UUID")]
    InvalidUuid {
        /// Which identifier failed, e.g. `"ProjectId"`.
        kind: &'static str,
    },

    /// An identifier that must be non-empty was empty or whitespace-only.
    #[error("invalid {kind}: must not be empty")]
    Empty {
        /// Which identifier failed, e.g. `"ModelId"`.
        kind: &'static str,
    },

    /// A `ModelId` was not of the form `provider/model-name`.
    #[error("invalid ModelId `{value}`: expected `provider/model-name`")]
    MalformedModelId {
        /// The rejected value. Model ids are non-secret registry slugs.
        value: String,
    },

    /// A `ProjectName` violated the filesystem-safe character rule.
    #[error(
        "invalid ProjectName `{value}`: must start with a letter or digit and \
         contain only letters, digits, `_` or `-` (max 63 characters)"
    )]
    InvalidProjectName {
        /// The rejected value. Project names are user-visible labels, not secrets.
        value: String,
    },

    /// An index kind string matched none of the known algorithms.
    #[error("unknown index kind `{value}`: expected brute, hnsw, ivf, bq or auto")]
    UnknownIndexKind {
        /// The rejected value.
        value: String,
    },

    /// A `ProjectTopology` had a zero replica or shard count.
    #[error("invalid topology: replicas={replicas}, shards={shards} (both must be >= 1)")]
    InvalidTopology { replicas: u8, shards: u8 },

    /// A name is representable but violates the stricter new-project policy.
    ///
    /// Distinct from [`DomainError::InvalidProjectName`]: that means "this can
    /// never be a project name", this means "this may not be a *new* one".
    #[error("project name `{value}` is not allowed for a new project: {reason}")]
    ProjectNamePolicy {
        /// The rejected value. Project names are user-visible labels, not secrets.
        value: String,
        /// Which clause of the creation policy failed.
        reason: &'static str,
    },

    /// An `ApiProject` carried an `is_cluster` flag contradicting `replicas`.
    #[error(
        "inconsistent topology: is_cluster={is_cluster} but replicas={replicas} \
         (is_cluster must equal replicas > 1)"
    )]
    InconsistentTopologyFlag { is_cluster: bool, replicas: u8 },
}
