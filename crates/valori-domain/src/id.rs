// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Cross-boundary identity types.
//!
//! # Admission rule
//!
//! An identifier is defined here only when it is **already represented in two
//! or more systems, incompatibly**. Each type below names its real consumers.
//! An identifier with one consumer belongs to that consumer's crate; an
//! identifier with no consumer is not built (see "Deliberately absent").
//!
//! # Two representations, on purpose
//!
//! These are not all the same shape, because the things they identify are not
//! all the same kind of thing:
//!
//! | Shape | Types | Why |
//! |---|---|---|
//! | UUID | [`ProjectId`], [`SessionId`], [`InstallationId`], [`CredentialRef`] | Minted by Valori, opaque, no meaning beyond identity |
//! | Slug | [`ModelId`] | Minted by a model registry; `openai/text-embedding-3-small` is the identity users type and read |
//! | Opaque handle | [`SnapshotId`] | Wraps a storage-owned object key; Valori does not mint it |
//!
//! Forcing all three into UUIDs would have required rewriting the model
//! registry and the object-store layout for no benefit.
//!
//! # Wire compatibility
//!
//! Every type here is `#[serde(transparent)]`: it serializes as the primitive
//! it wraps. This is deliberate and load-bearing — it is what allows these
//! types to replace today's raw `String` fields (for example
//! `ProjectManifest.id`) **without changing a single byte of any existing
//! `project.json`, HTTP response, or Cloud row.**
//!
//! # Deliberately absent
//!
//! - **`RuntimeId`** — there is no runtime identity to name. `valori_daemon::Runtime`
//!   is keyed by `kind() -> &'static str` and there is exactly one implementor
//!   (`LocalRuntime`); nodes are addressed by project name. Add this when a
//!   second runtime backend (Docker, SSH, hosted) exists *and* runtimes need to
//!   be addressed individually rather than by kind.
//! - **`PipelineId`** — there is no `Pipeline` platform primitive.
//!   `valori_ingest::PipelineConfig` and `PipelineResult` are ingest-local and
//!   never addressed by id. Add this when pipelines become durable,
//!   user-addressable objects — and build them on the existing
//!   `Operation → Planner → ExecutionGraph → Executor` model, never as a second
//!   orchestration engine. See `docs/architecture/ownership.md`.
//! - **Cloud identity** — `OrganizationId`, `UserId`, `BillingAccountId`,
//!   `SubscriptionId`, `DeploymentId`, `WorkerId` belong to the private Cloud
//!   control plane. Enforced by `dependency_direction.rs`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validate::validating_deserialize;

use crate::error::{DomainError, Result};

// ── Re-exports from valori-core ───────────────────────────────────────────────
//
// These are kernel identity types that also cross product boundaries. They are
// RE-EXPORTED, never redefined — there is exactly one `CollectionId` type in
// the workspace, and it is `valori_core::NamespaceId`. Re-exporting lets a
// consumer such as `valori-daemon` speak the full platform vocabulary through
// one dependency instead of two.

pub use valori_core::{CollectionId, ExecutionId, NamespaceId};

// ── ProjectId ─────────────────────────────────────────────────────────────────

/// The logical identity of a Valori project.
///
/// # What it represents
///
/// A project is a user's isolated data store: one kernel state, one event log,
/// one set of collections. `ProjectId` is the **logical** identity of that
/// project and the only thing that survives every representation of it.
///
/// # What it is not
///
/// This distinction is the whole point of the type, and it matters as soon as
/// local projects, Cloud projects, sync and migration coexist:
///
/// - **The filesystem path is not the identity.** A project can be moved,
///   restored to a different directory, or mounted at a different root.
/// - **The database row is not the identity.** The same project may exist in a
///   local `project.json`, in the daemon's redb registry, and in a Cloud row.
/// - **The display name is definitely not the identity.** `name` is a mutable
///   label; two projects in different workspaces may share one.
///
/// # Guarantees
///
/// - Stable for the lifetime of the project, across renames, moves and restores.
/// - Globally unique without coordination (UUID v4, 122 bits of randomness).
/// - Opaque: no information may be inferred from the bytes, and no ordering is
///   meaningful. Do not sort by it or parse structure out of it.
///
/// # Not guaranteed
///
/// - **Not sequential and not time-ordered.** Use `created_at` to order projects.
/// - **Not a secret.** It appears in URLs and logs. Authorization is a Cloud
///   concern and is never implied by possession of an id.
/// - **Not populated on legacy records.** Projects created before the id
///   existed are keyed by name; resolving those is the adapter's job, not this
///   type's. See `ARCHITECTURE_AUDIT.md` §9 and migration risk R2.
///
/// # Consumers
///
/// `valori-daemon` (`ProjectManifest.id`, a UUID `String` today),
/// `valori-metadata` (keyed on `name` today), `ui/src/lib/server/projects.ts`,
/// and the Cloud `projects` table. Four representations, one meaning.
///
/// # Example
///
/// ```
/// use valori_domain::ProjectId;
///
/// let id = ProjectId::new();
/// let text = id.to_string();
///
/// // Round-trips through the wire form used by project.json and HTTP.
/// assert_eq!(id, text.parse::<ProjectId>().unwrap());
/// assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{text}\""));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(Uuid);

// ── SessionId ─────────────────────────────────────────────────────────────────

/// One run of a Valori application, from launch to exit.
///
/// # What it represents
///
/// A single application session. Every event emitted by one process run — by
/// the Rust side or the JavaScript side — carries the same `SessionId`, which
/// is what makes a session reconstructable from an event stream.
///
/// # Guarantees
///
/// Stable for the lifetime of one process run; distinct across runs.
///
/// # Not guaranteed
///
/// - **Not stable across restarts.** That is [`InstallationId`]'s job.
/// - **Not an identity.** A session says nothing about *who* is using the app.
///   Correlating sessions to people is a Cloud concern and requires consent.
///
/// # Consumers
///
/// `desktop/src-tauri/src/telemetry.rs` (a UUID `String` today),
/// `ui/src/lib/telemetry.ts`, and the Cloud telemetry ingest endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Uuid);

// ── InstallationId ────────────────────────────────────────────────────────────

/// One installation of a Valori application on one machine.
///
/// # What it represents
///
/// A persistent, locally generated identifier that survives restarts and
/// updates. It is what distinguishes "one user launched the app forty times"
/// from "forty users launched it once".
///
/// # Guarantees
///
/// Stable across process restarts and application updates; local to one machine.
///
/// # Not guaranteed
///
/// - **Not a user identity, and must not be treated as one.** It is not tied to
///   an account and must never be used for authorization.
/// - **Not stable across reinstalls** or across machines.
///
/// # Consumers
///
/// `desktop/src-tauri/src/telemetry.rs` (a `String` today), the queued
/// `events.jsonl` envelope, and the Cloud telemetry ingest endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstallationId(Uuid);

// ── CredentialRef ────────────────────────────────────────────────────────────

/// An opaque reference to a provider credential (e.g. an OpenAI/Cohere API
/// key) stored in the OS credential store. Never the secret itself.
///
/// # What it represents
///
/// The join key between Studio's persisted provider configuration
/// (`provider`, `model`, `credential_ref`) and the actual secret, which
/// lives only in the OS keychain (`CredentialService`,
/// `desktop/src-tauri/src/credential_service.rs`). See
/// `docs/reviews/studio-credentials-audit.md` and
/// `docs/phases/phase-studio-S3-credentials.md`.
///
/// # Guarantees
///
/// - Opaque: carries no information about the provider, the secret, or
///   anything else — a random UUID v4, exactly like [`InstallationId`].
/// - Safe to persist anywhere (`studio.redb`, `project.json`, logs,
///   telemetry) — it is a reference, never the credential contents.
/// - One `CredentialRef` names exactly one OS-keychain entry.
///
/// # Not guaranteed
///
/// - **Not the secret**, and cannot be turned into one without a live
///   `CredentialService::get` call against the OS credential store.
/// - **Not portable across machines.** The OS keychain entry it points to
///   is machine-scoped; a `CredentialRef` copied to another machine (e.g.
///   via a `studio.redb` backup restored elsewhere) will not resolve there.
///   See the S3 phase doc's backup/recovery section.
/// - **Not a `CredentialKind`/`CredentialScope` system.** Only one
///   credential shape (`provider + API key`) exists in this codebase today;
///   do not read more structure into this type than that.
///
/// # Consumers
///
/// `desktop/src-tauri` (`CredentialService`, the Tauri credential commands),
/// `ui/src/lib/hooks/{useLLMConfig,useEmbeddingConfig}.ts` and
/// `SettingsModal.tsx`'s reranker config (a UUID `String` on the JS side).
///
/// # Example
///
/// ```
/// use valori_domain::CredentialRef;
///
/// let a = CredentialRef::new();
/// let b = CredentialRef::new();
/// assert_ne!(a, b, "distinct credentials must never share a reference");
///
/// let text = a.to_string();
/// assert_eq!(a, text.parse::<CredentialRef>().unwrap());
/// assert_eq!(serde_json::to_string(&a).unwrap(), format!("\"{text}\""));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialRef(Uuid);

// ── ModelId ───────────────────────────────────────────────────────────────────

/// A model in the Valori model registry, as `provider/model-name`.
///
/// # What it represents
///
/// The stable slug identifying a model across the registry, the on-disk package
/// store, node configuration and the UI — for example
/// `openai/text-embedding-3-small` or `ollama/nomic-embed-text`.
///
/// # Why a slug and not a UUID
///
/// The identity is the thing users type, read in config, and see in the model
/// picker. It is minted by external registries, not by Valori. Replacing it
/// with a UUID would add a lookup table and break every existing
/// `ModelManifest.id` on disk for no gain.
///
/// # Guarantees
///
/// - Always contains exactly one `/`, with a non-empty provider and name.
/// - Compares and hashes by exact bytes. Comparison is **case-sensitive**:
///   registries are case-sensitive and normalising here would silently alias
///   two distinct upstream models.
///
/// # Not guaranteed
///
/// - **Does not include a version.** `ModelManifest.version` is a separate,
///   optional field. Two artifacts of the same model share a `ModelId`.
/// - **Does not imply the model exists**, is installed, or is supported by any
///   runtime. It is a name, not a handle. Resolution is `valori-models`' job.
/// - **The provider segment is not a `valori_models::ProviderKind`** — it is
///   free text so third-party registries are expressible.
///
/// # Consumers
///
/// `valori-models` (`ModelManifest.id`, a `String` today), `valori-daemon`
/// (whose only workspace dependency is `valori-models`), node embed
/// configuration, and the Studio/Cloud model settings UI.
///
/// # Errors
///
/// [`DomainError::Empty`] for blank input; [`DomainError::MalformedModelId`]
/// when the `provider/name` shape is not met.
///
/// # Example
///
/// ```
/// use valori_domain::ModelId;
///
/// let id: ModelId = "openai/text-embedding-3-small".parse().unwrap();
/// assert_eq!(id.provider(), "openai");
/// assert_eq!(id.name(), "text-embedding-3-small");
///
/// assert!("no-slash".parse::<ModelId>().is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ModelId(String);

// `Deserialize` routes through `ModelId::parse` — see `crate::validate`.
validating_deserialize!(ModelId);

// ── SnapshotId ────────────────────────────────────────────────────────────────

/// An opaque handle to one snapshot in an object store.
///
/// # What it represents
///
/// The object key of a snapshot, e.g.
/// `snapshots/00000001750000000_abc12345.snap`. It is passed from a listing
/// (`GET /v1/storage/snapshots`) back into a restore
/// (`POST /v1/storage/snapshots/restore`), through the UI and the SDKs.
///
/// # This is a handle, not an identity
///
/// Unlike [`ProjectId`], Valori does **not** mint this value. The key format is
/// owned by `valori_storage::object_store`, which sits behind the domain
/// firewall and cannot depend on this crate. `SnapshotId` exists so the
/// **boundary** layers — HTTP, UI, SDK, Cloud — stop passing bare `String`s
/// that could be any key at all.
///
/// # Guarantees
///
/// - Non-empty.
/// - Round-trips byte-for-byte: whatever the storage layer emitted is exactly
///   what a restore receives. This crate never rewrites, normalises or
///   canonicalises the key.
///
/// # Not guaranteed
///
/// - **No structure may be parsed out of it.** The `{epoch}_{hash8}.snap`
///   shape is a storage implementation detail and may change; do not sort by
///   it, and do not extract the epoch. Use snapshot listing metadata instead.
/// - **Does not imply the snapshot exists** or is readable. It is a key.
/// - **Not portable across object stores or prefixes.**
///
/// # Consumers
///
/// `valori-node`'s storage endpoints, `ui/src/app/api/storage/snapshots/*`, the
/// Studio snapshots page, the Python SDK's `restore_from_store(key=…)`, and
/// Cloud restore flows.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SnapshotId(String);

// `Deserialize` routes through `SnapshotId::parse` — see `crate::validate`.
validating_deserialize!(SnapshotId);

// ── UUID-backed implementations ───────────────────────────────────────────────

/// Implements the shared surface of a UUID-backed identifier.
///
/// A macro rather than a generic wrapper so each type stays a distinct nominal
/// type: passing a `SessionId` where a `ProjectId` is expected must not compile.
macro_rules! uuid_id {
    ($name:ident) => {
        impl $name {
            /// Mint a new random identifier (UUID v4).
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing UUID.
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The underlying UUID.
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// The nil identifier, for tests and sentinel values.
            pub const NIL: Self = Self(Uuid::nil());
        }

        impl Default for $name {
            /// Mints a **new random** identifier — not the nil value.
            ///
            /// Use [`Self::NIL`] when a placeholder is wanted.
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(s: &str) -> Result<Self> {
                Uuid::parse_str(s.trim())
                    .map(Self)
                    .map_err(|_| DomainError::InvalidUuid {
                        kind: stringify!($name),
                    })
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }
    };
}

uuid_id!(ProjectId);
uuid_id!(SessionId);
uuid_id!(InstallationId);
uuid_id!(CredentialRef);

// ── ModelId implementation ────────────────────────────────────────────────────

impl ModelId {
    /// Parse a `provider/model-name` slug.
    ///
    /// Surrounding whitespace is trimmed; nothing else is normalised.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value: String = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(DomainError::Empty { kind: "ModelId" });
        }
        match trimmed.split_once('/') {
            Some((provider, name)) if !provider.is_empty() && !name.is_empty() => {
                Ok(Self(trimmed.to_string()))
            }
            _ => Err(DomainError::MalformedModelId {
                value: trimmed.to_string(),
            }),
        }
    }

    /// The provider segment, before the `/`.
    pub fn provider(&self) -> &str {
        self.0.split_once('/').map(|(p, _)| p).unwrap_or(&self.0)
    }

    /// The model-name segment, after the `/`.
    pub fn name(&self) -> &str {
        self.0.split_once('/').map(|(_, n)| n).unwrap_or("")
    }

    /// The full slug.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ModelId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

// ── SnapshotId implementation ─────────────────────────────────────────────────

impl SnapshotId {
    /// Wrap an object key produced by the storage layer.
    ///
    /// The key is stored verbatim — no trimming, no normalisation — because a
    /// restore must present byte-identical bytes to the object store.
    pub fn parse(key: impl Into<String>) -> Result<Self> {
        let key: String = key.into();
        if key.trim().is_empty() {
            return Err(DomainError::Empty { kind: "SnapshotId" });
        }
        Ok(Self(key))
    }

    /// The object key, verbatim.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SnapshotId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}
