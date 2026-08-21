// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! # valori-domain
//!
//! Cross-boundary domain vocabulary for the Valori platform.
//!
//! ## What this crate is
//!
//! `valori-domain` holds the small set of concepts that **more than one
//! product surface has to agree on**: the daemon, the node, the model manager,
//! Valori Studio, Valori Cloud, the CLI and the SDKs. It exists so that a
//! project, a model or a session means the same thing — and is spelled the
//! same way — on every side of every boundary.
//!
//! ## What this crate is not
//!
//! It is **not** a place to put every type in Valori. A type earns a place here
//! only when it is already represented in two or more systems, incompatibly.
//! If exactly one crate uses a concept, that concept belongs to that crate.
//!
//! It is **not** kernel vocabulary. `valori-kernel` is `no_std` and portable
//! (invariant #7); it must never learn what a project is. This crate sits
//! *beside* the kernel, not below it:
//!
//! ```text
//!            valori-core   (no_std, zero deps)
//!             │        │
//!  valori-kernel      valori-domain   ← you are here (std)
//!  (no_std, portable)  │
//!                      ├─▶ valori-models ─▶ valori-ingest
//!                      ├─▶ valori-daemon ─▶ desktop (Tauri)
//!                      └─▶ valori-engine ─▶ valori-node
//! ```
//!
//! Nothing in the determinism-critical path — kernel, wire, storage, state,
//! index, rag, verify — may reach this crate, even transitively. Those crates
//! own the snapshot V6, event-log V4 and BLAKE3 audit-chain formats frozen by
//! `COMPATIBILITY.md`, and product vocabulary must never influence those bytes.
//! The rule is enforced by `crates/valori-node/tests/dependency_direction.rs`.
//!
//! ## What is not here, and why
//!
//! **Cloud identity.** `OrganizationId`, `UserId`, `BillingAccountId`,
//! `SubscriptionId`, `DeploymentId` and `WorkerId` belong to the private Cloud
//! control plane. A local Studio project has no organization and no user — a
//! user can run Valori entirely offline and never authenticate. Putting these
//! here would make the open-source platform carry commercial vocabulary for
//! zero open-source benefit. `dependency_direction.rs` fails the build if any
//! of them is defined in this crate.
//!
//! **Kernel identity.** `RecordId`, `NodeId`, `EdgeId`, `NamespaceId`,
//! `ShardId` and `ClusterEpoch` live in [`valori_core`] and are re-exported
//! here where they cross a product boundary — see [`id`]. They are re-exported,
//! never redefined: there is exactly one `CollectionId` type in the workspace.
//!
//! **Speculative identity.** `RuntimeId` and `PipelineId` are deliberately
//! absent — see [`id`] for the reasoning and the conditions under which each
//! should be added.
//!
//! ## Stability and versioning
//!
//! Every type here is part of a wire contract: it appears in `project.json`
//! manifests on disk, in HTTP JSON, and in Cloud persistence. Consequently:
//!
//! - Every ID serializes **transparently**, as the primitive it wraps. A
//!   `ProjectId` is `"3f1a…"` on the wire, never `{"id":"3f1a…"}`. This is what
//!   lets these types replace today's raw `String` fields without a format
//!   change or a migration.
//! - Changing a representation is a breaking change governed by
//!   `COMPATIBILITY.md`, not an implementation detail.
//! - Round-trip and wire-shape tests in `tests/` are the guard. Treat a failure
//!   there as a compatibility break, not a broken test.
//!
//! ## Concurrency
//!
//! Every type here is an immutable value: `Clone`, `Send`, `Sync`, with no
//! interior mutability and no global state. They are free to cross threads and
//! `await` points. UUID-backed IDs are additionally `Copy`.
//!
//! ## Error semantics
//!
//! Parsing is the only fallible operation. It returns [`DomainError`] and never
//! panics. Constructors that cannot fail take owned values directly.

#![forbid(unsafe_code)]

pub mod error;
pub mod id;
pub mod project;
pub(crate) mod validate;

pub use error::{DomainError, Result};
pub use id::{
    CollectionId, CredentialRef, ExecutionId, InstallationId, ModelId, NamespaceId, ProjectId,
    SessionId, SnapshotId,
};
pub use project::{
    ApiProject, IndexKind, LocalProject, Metric, Project, ProjectName, ProjectTopology, Timestamp,
};
