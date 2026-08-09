// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! The canonical Project domain model.
//!
//! # The problem this solves
//!
//! `ARCHITECTURE_AUDIT.md` §9 found four representations of "a project", with
//! three primary-key strategies, three time encodings, three type systems and
//! three different names for "how many replicas":
//!
//! | | `valori-daemon` | `valori-metadata` | `projects.ts` | Cloud |
//! |---|---|---|---|---|
//! | identity | `id: String` (UUID) | keyed on `name` | keyed on `name` | table row |
//! | replicas | `cluster: Option<..>` | `node_count` + `mode` | `replication: 1\|3` | — |
//! | shards | absent | `shard_count` | `shardCount` | — |
//! | index | `String` | `IndexKind` | string union | — |
//! | time | `u64` unix | `u64` unix | ISO string | — |
//!
//! # What this module is, and is not
//!
//! [`Project`] is the **domain** model: what a project *means*. It is
//! deliberately **not** the persistence model, the API model, or the UI model.
//! Each boundary keeps its own representation and converts explicitly:
//!
//! ```text
//!                      Project  (this module — meaning)
//!                         │
//!         ┌───────────────┼───────────────┬──────────────────┐
//!         ▼               ▼               ▼                  ▼
//!  ProjectManifest   metadata::Project  ApiProject      (TS) UiProject
//!  daemon/project.json   redb record    HTTP JSON     generated in M5
//!  + workspace           + record_count  (this module)  from ApiProject
//!  + restart_policy      + port
//!  + on-disk paths
//! ```
//!
//! The persistence models keep fields the domain does not have. That is correct
//! and intended: a restart policy is an operational property of one daemon's
//! copy of a project, not part of what a project *is*. Forcing all four into
//! one struct would produce a type with twenty `Option` fields and four sets of
//! serde attributes — the thing this design exists to avoid.
//!
//! # What is deliberately not in `Project`
//!
//! Each exclusion is a decision, not an oversight:
//!
//! | Field | Where it belongs | Why |
//! |---|---|---|
//! | `dir` / `path` | [`LocalProject`] | Location is not identity — see [`ProjectId`] |
//! | `port`, `nodes[]` | `valori_daemon::NodeInfo` | Runtime allocation, changes every start |
//! | `workspace` | daemon persistence | One consumer; a daemon-local grouping |
//! | `restart_policy` | daemon persistence | Operational policy, not project meaning |
//! | `mode` | derived | `ProjectTopology::is_cluster()` — storing it separately from `replicas` lets the two disagree |
//! | `maxRecords`, `collections` | TS manifest / node | One consumer each; collections are node state |
//! | `embedding` | *deferred* | The TS shape carries an `apiKey`. A shared model that can hold a secret needs a secrets decision first — see the M2 report |
//! | `organization_id`, `region`, `deployment_id` | **private Cloud** | A local project has no organization |

use std::fmt;
use std::num::NonZeroU8;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, Result};
use crate::id::ProjectId;
use crate::validate::validating_deserialize;

// ── ProjectName ───────────────────────────────────────────────────────────────

/// A project's human-readable label, validated as filesystem-safe.
///
/// # What it represents
///
/// The mutable display name. It is **not** the identity — see [`ProjectId`].
///
/// # Guarantees — the *compatibility* contract
///
/// A `ProjectName` accepts exactly what `valori_daemon::ProjectStore::is_valid_name`
/// accepts, because that is the validator that actually gated project creation
/// on disk:
///
/// - non-empty
/// - at most [`Self::MAX_LEN`] (64) bytes
/// - every character is ASCII alphanumeric, `_` or `-`
///
/// That character rule is what makes the name safe to use as a directory
/// name: `/`, `\` and `.` are all rejected, so `..`, `../x` and `/abs` cannot
/// be represented.
///
/// # Why this is not the stricter UI rule
///
/// `ui/src/lib/server/projects.ts::isValidName` is stricter — it additionally
/// requires the first character to be alphanumeric and caps length at 63. An
/// earlier revision of this type copied that stricter rule, which meant
/// projects the daemon had legitimately created (`_scratch`, `-tmp`, 64-char
/// names) could not be represented at all. Because `ProjectStore::list()`
/// skips projects that fail to load, those projects would have silently
/// **disappeared from the project list**. See `docs/reviews/m2-project-review.md`
/// finding F2.
///
/// A value object must be able to represent every value that legitimately
/// exists. Stricter rules for *new* names are a creation policy, not an
/// identity constraint — see [`ProjectName::check_new_project_policy`].
///
/// # Not guaranteed
///
/// - **Not unique on its own.** Two workspaces may hold the same name. Only
///   [`ProjectId`] is unique.
/// - **Not stable.** Renaming is allowed and does not change the id.
/// - **Not normalised.** The value is stored exactly as given; nothing is
///   trimmed, case-folded or rewritten. A name is a directory name, and
///   rewriting one would orphan the directory.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProjectName(String);

// `Deserialize` routes through `ProjectName::parse` — see `crate::validate`.
// Without this, `#[serde(transparent)]` would admit any string, including
// `"../../etc/passwd"` (review finding F1).
validating_deserialize!(ProjectName);

impl ProjectName {
    /// Maximum length in bytes, matching `valori_daemon::ProjectStore::is_valid_name`.
    pub const MAX_LEN: usize = 64;

    /// Maximum length accepted by the stricter new-project creation policy.
    ///
    /// Matches `ui/src/lib/server/projects.ts::isValidName`.
    pub const NEW_PROJECT_MAX_LEN: usize = 63;

    /// Validate and wrap a project name against the compatibility contract.
    ///
    /// Accepts every name an existing daemon-created project may carry. Use
    /// [`Self::check_new_project_policy`] to additionally apply the stricter
    /// rule for names being created for the first time.
    ///
    /// # Errors
    ///
    /// [`DomainError::Empty`] when blank, [`DomainError::InvalidProjectName`]
    /// when the length or character rule is not met.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value: String = value.into();
        if value.is_empty() {
            return Err(DomainError::Empty {
                kind: "ProjectName",
            });
        }
        // Byte length is compared against a char rule that admits ASCII only,
        // so bytes and characters coincide for every accepted value.
        let valid = value.len() <= Self::MAX_LEN
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

        if valid {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidProjectName { value })
        }
    }

    /// Apply the stricter policy for a **newly created** project.
    ///
    /// This is a policy check, deliberately separate from [`Self::parse`]: it
    /// constrains what may be created, never what may be represented. Calling
    /// it on a pre-existing project is meaningless and may legitimately fail.
    ///
    /// The rule matches the UI validator: first character alphanumeric, at most
    /// [`Self::NEW_PROJECT_MAX_LEN`] characters.
    ///
    /// # Errors
    ///
    /// [`DomainError::ProjectNamePolicy`] describing which clause failed.
    pub fn check_new_project_policy(&self) -> Result<()> {
        if self.0.len() > Self::NEW_PROJECT_MAX_LEN {
            return Err(DomainError::ProjectNamePolicy {
                value: self.0.clone(),
                reason: "new project names may be at most 63 characters",
            });
        }
        if !self
            .0
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        {
            return Err(DomainError::ProjectNamePolicy {
                value: self.0.clone(),
                reason: "new project names must start with a letter or digit",
            });
        }
        Ok(())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProjectName {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

// ── IndexKind ─────────────────────────────────────────────────────────────────

/// The vector index algorithm a project's node runs.
///
/// # Guarantees
///
/// Parsing and rendering are byte-compatible with the three forms in use today:
/// `valori_metadata::IndexKind` (enum), `ProjectManifest.index` (`String`), and
/// the TypeScript string union. `"bruteforce"` and `"mstg"` are accepted as
/// input aliases for `brute` and `auto`, matching
/// `valori_node::config`'s `VALORI_INDEX` parsing.
///
/// # Not guaranteed
///
/// - **Not mutable after first insert.** Enforcement lives in the node, not here.
/// - **`Auto` is not a fourth algorithm.** It resolves at build time by record
///   count (brute < 10k, BQ 10k–2M, HNSW > 2M).
///
/// # Duplication note
///
/// `valori_metadata::IndexKind` still exists and is unchanged. This is a
/// **planned, temporary** duplication: `valori-metadata` adopts this type in
/// step M3, at which point its own enum is deleted. Until then the adapters
/// convert. Nothing is deleted in M2.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    #[default]
    Brute,
    Hnsw,
    Ivf,
    Bq,
    Auto,
}

impl IndexKind {
    /// The canonical lowercase tag — what is written to manifests and JSON.
    pub fn as_str(&self) -> &'static str {
        match self {
            IndexKind::Brute => "brute",
            IndexKind::Hnsw => "hnsw",
            IndexKind::Ivf => "ivf",
            IndexKind::Bq => "bq",
            IndexKind::Auto => "auto",
        }
    }
}

impl fmt::Display for IndexKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IndexKind {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "brute" | "bruteforce" => Ok(IndexKind::Brute),
            "hnsw" => Ok(IndexKind::Hnsw),
            "ivf" => Ok(IndexKind::Ivf),
            "bq" => Ok(IndexKind::Bq),
            "auto" | "mstg" => Ok(IndexKind::Auto),
            other => Err(DomainError::UnknownIndexKind {
                value: other.to_string(),
            }),
        }
    }
}

// ── ProjectTopology ───────────────────────────────────────────────────────────

/// How a project is spread across processes and Raft groups.
///
/// # What it represents
///
/// The single canonical answer to the question that today has three different
/// spellings: `cluster: Option<ClusterConfig>` (daemon), `node_count` + `mode`
/// (metadata), `replication: 1 | 3` (TypeScript).
///
/// # Guarantees
///
/// - `replicas >= 1` and `shards >= 1`, enforced by the type ([`NonZeroU8`]).
/// - Standalone versus cluster is **derived**, never stored. `mode` and
///   `node_count` cannot disagree because `mode` does not exist.
///
/// # Not guaranteed
///
/// - **`replicas` is not restricted to 1 or 3.** The TypeScript union allows
///   only those two, but RFC-0007 does not, and constraining it here would
///   make a legitimate 5-node cluster unrepresentable. Wizard-level choices
///   are a UI concern.
/// - **Shards are not "cluster only".** `VALORI_SHARD_COUNT` applies to
///   standalone nodes too (logical shards); the TypeScript comment claiming
///   otherwise describes that wizard, not the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectTopology {
    /// Number of Raft replica nodes. `1` = standalone.
    pub replicas: NonZeroU8,
    /// Number of independent shards (Raft groups) each node runs.
    pub shards: NonZeroU8,
}

impl ProjectTopology {
    /// The default: one node, one shard.
    pub const STANDALONE: Self = Self {
        replicas: NonZeroU8::MIN,
        shards: NonZeroU8::MIN,
    };

    /// Build a topology, rejecting zero values.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTopology`] when either value is zero.
    pub fn new(replicas: u8, shards: u8) -> Result<Self> {
        match (NonZeroU8::new(replicas), NonZeroU8::new(shards)) {
            (Some(replicas), Some(shards)) => Ok(Self { replicas, shards }),
            _ => Err(DomainError::InvalidTopology { replicas, shards }),
        }
    }

    /// `true` when more than one replica participates — i.e. Raft is running.
    pub fn is_cluster(&self) -> bool {
        self.replicas.get() > 1
    }
}

impl Default for ProjectTopology {
    fn default() -> Self {
        Self::STANDALONE
    }
}

// ── Timestamp ─────────────────────────────────────────────────────────────────

/// Unix seconds since the epoch.
///
/// # Why unix seconds and not RFC 3339
///
/// Two of the three existing Rust representations already use `u64` unix
/// seconds; only the legacy TypeScript manifest uses ISO strings. Encoding a
/// wall-clock instant as an integer keeps this crate free of a date-library
/// dependency, and the ISO form is a *presentation* concern the UI adapter
/// handles (`new Date(secs * 1000).toISOString()`).
///
/// # Not guaranteed
///
/// - **Not monotonic.** It is wall clock; it can move backwards across a clock
///   adjustment. Do not use it to order events within an execution — that is
///   what `ExecutionId` and the audit chain's log index are for.
/// - **No sub-second precision.**
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Wrap a unix-seconds value.
    pub const fn from_unix_secs(secs: u64) -> Self {
        Self(secs)
    }

    /// The unix-seconds value.
    pub const fn as_unix_secs(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Project ───────────────────────────────────────────────────────────────────

/// What a Valori project *is*, independent of where it is stored or shown.
///
/// # What it represents
///
/// A project is a user's isolated data store: one kernel state, one event log,
/// one set of collections, one immutable vector dimension. This struct carries
/// exactly the properties that every surface — daemon, control plane, HTTP API,
/// Studio, Cloud — must agree on.
///
/// # Guarantees
///
/// - [`Project::id`] is the identity and never changes.
/// - Every field is validated at construction: no empty names, no zero
///   replicas, no unknown index kinds.
/// - Contains no secrets, no filesystem paths, and no runtime state, so it is
///   safe to serialize into an API response as-is.
///
/// # Not guaranteed
///
/// - **Not a live view.** It is a value, not a handle; `record_count` is a
///   last-known figure and `last_opened_at` may be stale.
/// - **Does not imply existence on disk or in a database.**
/// - **Not the persistence format.** Adapters convert; see the module docs.
///
/// # Concurrency
///
/// An immutable value: `Clone + Send + Sync`, no interior mutability.
///
/// # Example
///
/// ```
/// use valori_domain::{IndexKind, Project, ProjectId, ProjectName, ProjectTopology, Timestamp};
///
/// let project = Project {
///     id: ProjectId::new(),
///     name: ProjectName::parse("research-notes")?,
///     dim: 384,
///     index: IndexKind::Auto,
///     topology: ProjectTopology::STANDALONE,
///     created_at: Timestamp::from_unix_secs(1_750_000_000),
///     last_opened_at: None,
///     record_count: None,
/// };
///
/// assert!(!project.topology.is_cluster());
/// # Ok::<(), valori_domain::DomainError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// The logical identity. Stable across renames, moves and restores.
    pub id: ProjectId,
    /// Mutable, filesystem-safe display label.
    pub name: ProjectName,
    /// Vector dimension. Immutable after the first insert (enforced by the node).
    pub dim: u32,
    /// Index algorithm.
    pub index: IndexKind,
    /// Replica and shard counts.
    pub topology: ProjectTopology,
    /// When the project was created.
    pub created_at: Timestamp,
    /// When it was last opened, if ever.
    pub last_opened_at: Option<Timestamp>,
    /// Approximate record count at last close. `None` = unknown.
    ///
    /// Cosmetic: shown in listings, never used for routing or capacity checks.
    pub record_count: Option<u64>,
}

// ── LocalProject ──────────────────────────────────────────────────────────────

/// A [`Project`] together with where it lives on this machine.
///
/// # Why this is a separate type
///
/// The filesystem path is **not** the identity. A project can be moved,
/// restored into a different directory, or mounted at a different root, and it
/// is still the same project. Keeping the path out of [`Project`] means:
///
/// - the same `Project` value describes a local project and its Cloud twin;
/// - an API response cannot leak a filesystem path by accident;
/// - a moved directory is a change of `root`, not a change of identity.
///
/// # The Cloud counterpart
///
/// `CloudProject` is the mirror of this type and lives in the **private Cloud
/// repository**, not here — it composes a [`Project`] with `OrganizationId`,
/// `region` and `DeploymentId`, none of which may appear in an open-source
/// crate (`dependency_direction.rs` enforces this):
///
/// ```text
/// LocalProject { project, root }                          ← OSS, this crate
/// CloudProject { project, organization_id, region, .. }    ← private Cloud
/// ```
///
/// Both share `project.id`, which is exactly what makes local↔cloud sync and
/// migration expressible later.
///
/// # Not guaranteed
///
/// - **`root` is not validated and may not exist.** Checking the filesystem is
///   the daemon's job; this crate performs no I/O.
/// - **`root` is not canonicalised.** It is stored as given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalProject {
    /// The project itself.
    pub project: Project,
    /// Absolute path to the project's data directory.
    pub root: PathBuf,
}

impl LocalProject {
    pub fn new(project: Project, root: impl Into<PathBuf>) -> Self {
        Self {
            project,
            root: root.into(),
        }
    }

    /// The project's identity — shorthand for `self.project.id`.
    pub fn id(&self) -> ProjectId {
        self.project.id
    }

    /// The data directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

// ── ApiProject ────────────────────────────────────────────────────────────────

/// The wire representation of a [`Project`] over HTTP.
///
/// # What it represents
///
/// The one shape that the node, the daemon, the Cloud API, the TypeScript
/// client and the Python SDK all agree on. It exists as a type distinct from
/// [`Project`] so that the domain model can evolve without silently changing a
/// public API — and so that adding a field to the API is a visible, reviewable
/// act.
///
/// # Guarantees
///
/// - Field names are `snake_case` and stable; see `COMPATIBILITY.md`.
/// - `created_at` / `last_opened_at` are **unix seconds**, not ISO strings.
///   The legacy TypeScript manifest stores ISO; that is a property of that
///   persistence format, not of this API, and its adapter converts.
/// - Contains no secrets and no filesystem paths.
///
/// # Not guaranteed
///
/// - **Not additive-only forever.** Removing or retyping a field is a breaking
///   change and requires a version bump under `COMPATIBILITY.md`.
/// - **Not yet the generated TypeScript source.** Code generation is step M5;
///   until then the TS interface is written by hand to match this struct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiProject {
    pub id: ProjectId,
    pub name: ProjectName,
    pub dim: u32,
    pub index: IndexKind,
    /// `1` for standalone.
    pub replicas: u8,
    pub shards: u8,
    /// Derived from `replicas`; sent so clients need no branching logic.
    pub is_cluster: bool,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds; absent if never opened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_opened_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<u64>,
}

impl From<&Project> for ApiProject {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            dim: p.dim,
            index: p.index,
            replicas: p.topology.replicas.get(),
            shards: p.topology.shards.get(),
            is_cluster: p.topology.is_cluster(),
            created_at: p.created_at.as_unix_secs(),
            last_opened_at: p.last_opened_at.map(Timestamp::as_unix_secs),
            record_count: p.record_count,
        }
    }
}

impl TryFrom<ApiProject> for Project {
    type Error = DomainError;

    fn try_from(a: ApiProject) -> Result<Self> {
        let topology = ProjectTopology::new(a.replicas, a.shards)?;

        // `is_cluster` is derived from `replicas`, so a payload carrying both
        // can contradict itself. Silently trusting `replicas` and discarding
        // the flag would let a client believe it had requested a cluster while
        // getting a standalone project. Reject instead (review finding F5).
        if a.is_cluster != topology.is_cluster() {
            return Err(DomainError::InconsistentTopologyFlag {
                is_cluster: a.is_cluster,
                replicas: a.replicas,
            });
        }

        Ok(Project {
            id: a.id,
            name: a.name,
            dim: a.dim,
            index: a.index,
            topology,
            created_at: Timestamp::from_unix_secs(a.created_at),
            last_opened_at: a.last_opened_at.map(Timestamp::from_unix_secs),
            record_count: a.record_count,
        })
    }
}
