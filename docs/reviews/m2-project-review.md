# M2 review — canonical `Project` and adapters

**Reviewed:** `crates/valori-domain/src/{project.rs,id.rs,error.rs}`,
`crates/valori-daemon/src/domain_adapter.rs`,
`crates/valori-metadata/src/domain_adapter.rs`, and the three test suites
**Against:** `crates/valori-daemon/src/project.rs`, `crates/valori-metadata/src/project.rs`,
`ui/src/lib/server/projects.ts`
**Date:** 2026-08-08
**Verdict at review time:** do not start M3; two defects must be fixed first.
**Status now: M2.1 has landed — F1–F7 are all resolved.** See
[M2.1 outcome](#m21-outcome) at the end of this document. M3 remains not
started, and no consumer has been migrated.

No source was modified during this review. Behavioural claims below were
verified by executing the published crate from a throwaway binary outside the
repository, not by reading alone; that scratch crate has been deleted.

---

## Summary

The structural decisions hold up. Domain / persistence / API separation is real,
the metadata adapter's refusal to invent identity is the strongest thing in the
change, and `ProjectTopology` genuinely removes a class of bug.

Two defects undermine stated guarantees:

| # | Severity | Defect | M2.1 status |
|---|---|---|---|
| **F1** | **Critical** | `#[serde(transparent)]` bypasses **all** newtype validation. `ProjectName`, `ModelId` and `SnapshotId` accept anything through `Deserialize`, including `"../../etc/passwd"`. The documented guarantee "every field is validated at construction" is false on the deserialization path — which is the path untrusted input actually takes. | ✅ Fixed |
| **F2** | **High** | `valori_daemon::ProjectStore::is_valid_name` accepts names `ProjectName::parse` rejects. Projects that exist on disk today become unloadable the moment M3 routes reads through the adapter. | ✅ Fixed |
| **F3** | **High** | Pre-existing, inherited: a manifest without an `id` mints a **fresh UUID on every read**. `ProjectId` is not yet stable, which is the premise the whole model rests on. | ✅ Fixed |
| F4 | Medium | `manifest_from_domain` cannot demote a cluster to standalone; the doc comment claims it can. | ✅ Fixed |
| F5 | Medium | `ApiProject.is_cluster` is deserialized, may contradict `replicas`, and is then silently ignored. | ✅ Fixed |
| F6 | Medium | `dim` is `usize` / `u32` / `u16` across the three layers, and both adapters **saturate silently**. | ✅ Fixed |
| F7 | Medium | `index_from_domain` falls back to `Brute` on an unmatched variant — a silent change of index algorithm. | ✅ Fixed |
| F8 | Low | `Project` derives `Serialize`/`Deserialize`, inviting the direct persistence the design exists to prevent. | ⏸ Deferred |
| F9 | Low | `PartialEq` on `Project` is structural, so equality ≠ identity. | 📝 Documented |
| F10 | Low | `record_count` is a cached projection, not a stable domain concept. | 📝 Documented |
| F11 | Low | `Timestamp(pub u64)` exposes its field; every other newtype does not. | ⏸ Deferred |
| F12 | Low | `api_project_never_leaks_a_path_or_a_secret` is a substring assertion that proves nothing structural and can false-fail. | ✅ Fixed |
| F13 | Forward risk | Per-collection index/dimension is a known deferred direction; `Project.index` and `Project.dim` presume project-level. | 📝 Documented |
| F14 | Low | Two distinct types named `ProjectAdapterError`. | ⏸ Deferred |

---

## The two blocking defects, in detail

### F1 — validation is not enforced on deserialization (Critical)

`ProjectName`, `ModelId` and `SnapshotId` are `#[serde(transparent)]` newtypes
with private fields and validating `parse()` constructors. `serde` generates a
`Deserialize` impl that deserializes the inner primitive and wraps it. **The
constructor is never called.**

Observed, by running the crate:

```text
ProjectName "../../etc/passwd": parse_ok=false  serde_ok=true
ProjectName "a/b":              parse_ok=false  serde_ok=true
ProjectName "":                 parse_ok=false  serde_ok=true
ProjectName "-x":               parse_ok=false  serde_ok=true
ModelId     "no-slash":         parse_ok=false  serde_ok=true
SnapshotId  "":                 parse_ok=false  serde_ok=true
```

And end-to-end through the API type, with a hostile body:

```text
ApiProject hostile ACCEPTED: name="../../../etc/passwd"
domain Project hostile ACCEPTED: name="../../etc"
```

Why this matters concretely: `ProjectName` is used as a **directory name** by
all three existing implementations. The `project_name_rejects_path_traversal`
test asserts `ProjectName::parse` rejects `..`, and it does — but every
real-world entry point (an HTTP body, a `project.json`, a redb value) arrives
through `Deserialize`, which does not.

The topology field is *not* affected, and this is worth noting because it shows
the correct pattern already exists in the same file: `NonZeroU8` has a
validating `Deserialize`, so `{"replicas":0}` is rejected —

```text
topology zero via serde: Err("invalid value: integer 0, expected a nonzero u8")
```

`ProjectTopology` is safe because its *primitive* validates. The string newtypes
are unsafe because `String` accepts everything.

**Fix direction (M2 rework, before M3):** implement `Deserialize` manually — or
via `#[serde(try_from = "String")]` — so it routes through `parse()`. Then add
the missing tests: every validated newtype must have a "rejects invalid input
via serde" case, not only via `parse`. The three existing round-trip tests all
start from a *valid* value, which is why this was not caught.

### F2 — the daemon accepts names the domain model rejects (High)

| Validator | Rule |
|---|---|
| `valori_daemon::ProjectStore::is_valid_name` | non-empty, `len <= 64`, all `[A-Za-z0-9_-]` |
| `valori_domain::ProjectName::parse` | `len <= 63`, **first char alphanumeric**, all `[A-Za-z0-9_-]` |
| `ui/.../projects.ts::isValidName` | `^[a-zA-Z0-9](?:[a-zA-Z0-9_-]{0,62})$` |

`ProjectName` was modelled on the TypeScript regex. The daemon's own validator
— the one that actually gated project creation on disk — is **looser**:

```text
name "_scratch"   : daemon_accepts=true  domain_accepts=false
name "-tmp"       : daemon_accepts=true  domain_accepts=false
name <64 chars>   : daemon_accepts=true  domain_accepts=false
```

So a project legitimately created through the daemon API named `_scratch`
exists on disk right now, and `manifest_to_domain` returns
`InvalidName` for it. Worse, `ProjectStore::list()` swallows per-project errors
(`if let Ok(project) = self.get(name)`), so at M3 such a project would silently
**disappear from the project list** rather than surface an error.

**This must be decided, not patched:** either `ProjectName` widens to match the
daemon (accept leading `_`/`-`, 64 chars) and the UI validator tightens
separately, or M3 ships a rename migration. Widening is the safer default —
the domain model should describe what exists, and only then constrain what is
new. Note the error message in `project.rs` already says "<=64 chars", so 64 is
the daemon's documented contract.

### F3 — `ProjectId` is not yet stable for legacy manifests (High, inherited)

`ProjectManifest.id` carries `#[serde(default = "crate::new_id")]`, and
`ProjectStore::get()` deserializes without writing back:

```rust
let config: ProjectManifest = serde_json::from_slice(&bytes)?;   // fresh UUID if `id` absent
Ok(Project { config, dir: … })                                   // never persisted
```

For any `project.json` written before `id` existed, **every read produces a
different `ProjectId`**. `manifest_to_domain` faithfully propagates whatever it
was handed, so the domain model inherits an identity that changes per call.

This is pre-existing and not caused by M2 — but M2's entire design, and the
answer to questions 6 and 7 below, depend on it being false. M3 cannot correlate
local and cloud projects on an id that is regenerated on each load.

**Fix direction:** a one-time backfill that persists an `id` for every manifest
lacking one, before any consumer reads identity through the adapter. The
existing `m001_project_registry` migration is the precedent and the natural home.

---

## Answers to the fifteen questions

### 1. Does `Project` contain only stable domain concepts?

**Almost.** Six of the eight fields are unambiguously domain: `id`, `name`,
`dim`, `index`, `topology`, `created_at`.

Two are weaker:

- **`record_count`** — a cached, cosmetic, staleness-prone projection. Its own
  doc comment says "Cosmetic … never used for routing". It is a read-model
  concern, and it round-trips lossily: `manifest_to_domain` hardcodes `None`, so
  `domain → manifest → domain` silently discards it. **Recommend removing it**
  from `Project` and returning it in a listing/status projection instead.
- **`last_opened_at`** — operational telemetry rather than meaning. It is
  present in all three implementations, so keeping it is defensible, but it is
  the second-weakest member.

`topology` deserves a note: replica and shard counts are arguably *deployment
configuration* rather than what a project *is* (see question 4). It is kept
because all three implementations already treat it as project-level and
immutable-ish, and because unifying it removes a real class of bug.

### 2. Which fields are manifest/persistence-specific?

Correctly excluded from `Project`, and verified preserved by the round-trip test:

| Field | Owner |
|---|---|
| `workspace` | daemon manifest |
| `restart_policy` | daemon manifest |
| `storage` | daemon manifest |
| `embedding` (incl. `api_key_ref`) | daemon manifest |
| `cluster.nodes[]` (port allocations) | daemon manifest |
| `dir` | daemon `Project`, metadata record |
| `port` | metadata record |
| `maxRecords` | TS manifest only |
| `mode` | metadata record — now *derived*, correctly not stored |

One asymmetry: **`id` is persistence-specific today**, existing only in the
daemon manifest. The domain model promotes it to universal, which is right, but
that promotion is exactly what F3 blocks.

### 3. Which fields are Cloud-specific?

None are present, which is correct. `organization_id`, `region`,
`deployment_id`, `subscription_id`, and API-key/service-account associations all
belong to `CloudProject` in the private repository.

The boundary is mechanically enforced, not just documented:
`dependency_direction.rs::cloud_only_concepts_are_not_defined_in_oss_platform_core`
fails the build if any of those types is *defined* in `valori-core`,
`valori-kernel` or `valori-domain`.

The Supabase `projects` table schema is **not in this repository**, so whether
`ApiProject` is a superset, subset or mismatch of the Cloud row is currently
unknown. That is an open M3 input, not a finding.

### 4. Which fields are deployment-specific?

This is the least clean answer.

- **Clearly deployment, correctly excluded:** `port`, `nodes[]`, `restart_policy`,
  `dir`, `storage`.
- **Arguably deployment, currently included:** `topology` (replicas, shards),
  `index`, `dim`.

`dim` and `index` are engine configuration passed as `VALORI_DIM` /
`VALORI_INDEX`. They sit in `Project` because they are immutable after first
insert and therefore behave like identity-adjacent facts — you cannot change
them without rebuilding the project. That justification is sound **today** but
is not permanent (see F13).

`topology` is the borderline case: `replicas` is genuinely deployment shape,
while `shards` affects on-disk layout (`events-shardN.log`). Keeping them
together is defensible; splitting `replicas` out into a deployment model later
would not be a breaking domain change.

### 5. Which fields are collection-specific?

**None, correctly.** `collections?: string[]` from the TypeScript manifest was
excluded — collections are node state, discovered from
`events.namespaces.json`, not project configuration.

The forward risk is the inverse: `dim` and `index` are currently project-level,
but per-collection index selection is a known deferred direction. If that lands,
`Project.index` becomes a *default* rather than a fact, and the field will need
renaming or moving. Worth a comment in `project.rs` now so it is not discovered
by surprise.

### 6. Is `ProjectId` correctly the logical identity?

**The design is correct. The implementation is not yet true.**

Correct in design, and tested:

- `moving_a_project_does_not_change_its_identity` — `root` lives on
  `LocalProject`, never on `Project`
- `renaming_a_project_does_not_change_its_identity`
- `two_projects_may_share_a_name_but_never_an_identity`

Not yet true in practice:

- **F3** — legacy manifests regenerate the id on every read.
- **F9** — `Project` derives `PartialEq` structurally, so two values describing
  the same logical project (a local copy and its Cloud twin, differing only in
  `record_count`) compare unequal. Nothing in the type says "compare by id".
  Recommend either documenting that `==` means "identical value, not same
  project", or adding an explicit `same_project_as()`.

### 7. Can local and cloud projects represent the same logical project?

**Structurally yes; operationally not yet.**

The composition is right:

```text
LocalProject { project, root: PathBuf }                    OSS
CloudProject { project, organization_id, region, … }       private
```

Both embed the same `Project`, so `project.id` is the join key, and neither repo
has to import the other. `CloudProject` correctly does not exist here.

Three gaps before this is real:

1. **F3** — a join key that changes per read joins nothing.
2. There is no way to express *which* representation is authoritative, or that a
   local project is a cache of a cloud one. Correctly deferred — no sync feature
   exists — but it is the next thing this model will be asked for.
3. Divergent-by-design fields (`record_count`, `last_opened_at`) will differ
   between the two copies of one logical project, which is another argument for
   moving `record_count` out (question 1).

### 8. Does serialization preserve existing on-disk compatibility?

**Yes.** No file format, wire format or call site changed. Verified:

- `project.json` — untouched; the adapter reads and writes the existing
  `ProjectManifest` unchanged.
- redb control-plane schema — untouched.
- Snapshot V6 / event log V4 / audit chain — unreachable from `valori-domain` by
  construction, enforced by `determinism_crates_cannot_reach_valori_domain`.
- Every ID is `#[serde(transparent)]`, so `ProjectId` has the same JSON form as
  the `String` it will replace — pinned by
  `ids_deserialize_from_the_string_forms_already_on_disk`.
- `ApiProject` is not served by any handler yet, so no API changed.

Two caveats:

- **F1** means the new types are not *validating* anything on the
  deserialization path, so "compatible" currently also means "no safer".
- **F8** — `Project` itself derives `Serialize`/`Deserialize`. Nothing persists
  it today, but the derive is an open invitation to persist the domain model
  directly and bypass the adapters, which is precisely what the design forbids.
  No test pins `Project`'s own JSON shape, so such a use would be unguarded.
  Consider dropping the derive, or pinning the shape.

### 9. Does the daemon adapter preserve fields it previously owned?

**Yes for field content; no for one state transition.**

Preserved and tested by `round_trip_preserves_the_daemon_only_fields`:
`workspace`, `restart_policy`, `embedding`, `storage`, and — importantly —
`cluster.nodes[]` port allocations. The decision to make `manifest_from_domain`
*mutate an existing manifest* rather than implement `From<&Project>` is the
right call and is what makes this safe.

**F4 — the one gap:** the function only ever writes the cluster block when
`project.topology.is_cluster()`. There is no `else` clause. So converting a
cluster project to standalone leaves `cluster: Some { replication: 3 }` on disk,
and the next `manifest_to_domain` reports three replicas again — the write is
silently discarded. The doc comment states *"`cluster` is left as `None` for a
standalone topology"*, which describes behaviour the code does not have; it is
only left alone, never cleared.

Either clear the block explicitly or document that topology demotion is not
supported through this path. No test covers the transition, which is why the
comment and the code could disagree.

Two further notes:

- `record_count` is hardcoded to `None` on the way in. Correct — the manifest
  has no such field — but it makes `domain → manifest → domain` lossy.
- **F6** — `dim: u32::try_from(manifest.dim).unwrap_or(u32::MAX)` saturates
  silently, and the adjacent comment claims the conversion is "safe" and
  "explicit rather than `as`", while the reverse direction uses
  `project.dim as usize`. This is inconsistent with the adapter's own
  reject-don't-coerce stance three lines away, where `shard_count > 255` is
  rejected outright.

### 10. Does the metadata adapter avoid inventing identity?

**Yes — and this is the best decision in M2.**

```rust
pub fn record_to_domain(record: &Project, id: ProjectId) -> Result<DomainProject, …>
```

The control-plane record has no id. Rather than mint one internally — which
would yield a different identity on every read and silently destroy any future
local↔cloud correlation — the signature forces the caller to supply it. The
missing information is impossible to overlook, and it names precisely what M3
must solve.

`record_from_domain` also correctly recomputes `mode` from topology, repairing
records where `mode` and `node_count` already contradict each other
(`mode_is_recomputed_and_cannot_contradict_node_count`).

Two defects in the same file:

- **F7** — `index_from_domain` ends in `.unwrap_or_default()`, which returns
  `Brute` if the string does not match. Since `IndexKind` is immutable after
  first insert, silently rewriting a project's index to `Brute` is a
  data-corruption path. The comment argues a test would catch drift, but the
  production fallback is still silent. Prefer an exhaustive `match` (the
  compiler then enforces drift detection) or an error.
- **F6** — `record.dim = u16::try_from(project.dim).unwrap_or(u16::MAX)`
  saturates. `dim` is `usize` in the daemon, `u32` in the domain and `u16` in
  metadata; a value above 65535 silently becomes 65535.

### 11. Are any secrets still crossing the `Project`/domain boundary?

**No secret is in `Project`, `ApiProject` or `LocalProject` today.** `embedding`
was deliberately excluded from the canonical model for exactly this reason, and
`LocalProject.root` — the only sensitive-ish value — is structurally absent from
`ApiProject`.

Three things to keep watching:

1. The underlying problem is unresolved, not fixed. `daemon::EmbeddingConfig`
   stores `api_key_ref` (a reference — correct), while
   `ui/src/lib/server/projects.ts::ProjectEmbedConfig` stores `apiKey` (the key
   itself). M3 must not resolve that divergence by lifting the TypeScript shape
   into a shared model. **A secrets decision is a prerequisite for migrating
   embedding config, not a follow-up.**
2. **F12** — `api_project_never_leaks_a_path_or_a_secret` asserts the serialized
   JSON does not *contain the substrings* `"dir"`, `"path"`, `"api_key"`,
   `"port"`, etc. That is not a structural guarantee: it passes trivially for
   this fixture and would false-fail for a project legitimately named
   `airport` or `support`. Replace with an assertion over the serialized field
   name set.
3. `DomainError::InvalidProjectName` and `MalformedModelId` embed the rejected
   value in the message. Both are non-secret by nature (a label, a registry
   slug), and the error docs say so — acceptable, but worth not extending that
   pattern to future variants that could carry a token.

### 12. Are there fields whose types should be domain-specific rather than primitive?

Yes — four, in priority order:

| Field | Today | Should be | Why |
|---|---|---|---|
| `dim` | `u32` | `VectorDimension` (non-zero, bounded) | It is validated nowhere. `dim: 0` is representable in the domain model even though the daemon rejects it at creation (`"dim must be > 0"`). A newtype also gives the three-width mismatch (F6) one place to be resolved. |
| `record_count` | `Option<u64>` | remove, or `RecordCount` | See question 1 — if it stays, a named type documents that it is an estimate. |
| `replicas` / `shards` | `NonZeroU8` | `ReplicaCount` / `ShardCount` | They are already non-zero-safe, but interchangeable: `ProjectTopology::new(shards, replicas)` compiles with the arguments reversed. Distinct newtypes make that a compile error. |
| `Timestamp` | `Timestamp(pub u64)` | private field (F11) | Every other newtype in the crate hides its field; this one does not, so `Timestamp(0)` bypasses the constructor for no benefit. |

`ApiProject`'s use of raw `u8`/`u64` is deliberate and should stay — the wire
model is meant to be dumb.

### 13. What will break when M3 migrates consumers?

Ordered by likelihood of biting:

| # | Break | Trigger | Mitigation |
|---|---|---|---|
| 1 | **Projects vanish from listings** | F2 — a project named `_scratch`, `-tmp`, or 64 chars fails `ProjectName::parse`, and `ProjectStore::list()` swallows the error via `if let Ok(...)` | Reconcile the validators **before** M3; make `list()` surface adapter errors instead of skipping |
| 2 | **Identity churn** | F3 — manifests without `id` regenerate it per read | Backfill-and-persist migration first |
| 3 | **TypeScript field renames** | `ApiProject` uses `replicas`/`shards`/`created_at` where the UI reads `replication`/`shardCount`/`createdAt` | This is a real breaking change to the UI contract. Ship the TS types and the handler in one change (M5), not piecemeal |
| 4 | **Timestamp encoding flip** | The legacy TS manifest stores ISO strings; `ApiProject` sends unix seconds | Convert in the adapter; verify every UI date formatter |
| 5 | **`shard_count > 255` becomes unloadable** | Daemon stores `u32`; domain uses `u8` | Values in use are 1/2/4/8, so low risk — but the failure is a hard error, not a clamp. Confirm no manifest exceeds it before migrating |
| 6 | **Silent `dim` truncation** | F6 — saturation in both adapters | Make the conversions fallible |
| 7 | **`mode` rewritten on disk** | `record_from_domain` recomputes `mode` | Intended repair, but it *is* a write to existing records. Ship it deliberately, with a note |
| 8 | **Index silently reset to `Brute`** | F7 | Replace the fallback with an exhaustive match |
| 9 | **Cluster demotion silently ignored** | F4 | Clear the block or document the restriction |
| 10 | Two `ProjectAdapterError` types collide in a file that imports both | F14 | Import-alias, or rename to `ManifestAdapterError` / `RecordAdapterError` |

Not at risk: kernel formats, snapshot/WAL/audit chain, Python SDK and FFI (no
project-management surface), cluster routing.

### 14. What should be migrated first?

Fix M2 before migrating anything. Proposed order:

**Phase 0 — repair M2 (no consumers touched)**
1. **F1** — route every validated newtype's `Deserialize` through its
   constructor; add negative serde tests for each.
2. **F2** — decide and reconcile the name rule across daemon / domain / UI.
   Recommend widening `ProjectName` to the daemon's contract.
3. **F5, F6, F7, F4** — the silent-coercion and asymmetry fixes. Small, and each
   is a data-corruption path if left.

**Phase 1 — identity**
4. Backfill and persist `ProjectId` for every manifest lacking one (**F3**).
   Nothing downstream is trustworthy until this lands.

**Phase 2 — one consumer, read-only**
5. Daemon read path only: `ProjectStore::get`/`list` return the domain view,
   with adapter errors surfaced rather than swallowed. Writes stay as they are.
   This exercises the adapter against real on-disk data with a cheap rollback.

**Phase 3 — control plane**
6. Add `id` to the metadata record, backfilled from the daemon registry; adopt
   `valori_domain::IndexKind` and delete the local enum.

**Phase 4 — API, then UI**
7. Serve `ApiProject` from the daemon/node project routes.
8. Only then migrate `projects.ts`, together with the generated TS types (M5),
   as a single change.

Deletion of the duplicate implementations stays last, unchanged from the M2 plan.

### 15. What should explicitly remain outside the canonical `Project` model?

**Persistence-owned:** `workspace`, `restart_policy`, `storage`, `embedding`,
`cluster.nodes[]`, `dir`, `port`, `maxRecords`.

**Derived, never stored:** `mode` / `is_cluster` (compute from `replicas`),
`replication` and `shardCount` as separate spellings.

**Node/runtime state:** `collections[]`, live record counts, health, uptime,
pid, allocated ports. A project listing that wants these should compose a
separate status projection rather than widen `Project`.

**Location:** the filesystem path stays on `LocalProject`. This is the rule most
likely to be eroded by convenience — a reviewer should reject any patch that
adds a path to `Project`.

**Cloud-owned:** `organization_id`, `user_id`, `region`, `deployment_id`,
`subscription_id`, billing state, API keys, service accounts, IP allowlists.
Enforced by `dependency_direction.rs`.

**Secrets:** anything resembling a credential, in any form. The daemon's
`api_key_ref` indirection is the pattern to follow if embedding config is ever
promoted.

**Recommended additions to this list:** `record_count` (a projection, see
question 1) and — pending the per-collection index decision — an explicit note
that `index` and `dim` are project-level *for now*.

---

## What M2 got right

Worth recording so it is not undone during the repairs:

- **The metadata adapter's forced `ProjectId` argument.** The single best
  decision in the change; it converts an invisible assumption into a compile error.
- **`manifest_from_domain` mutating rather than constructing.** Prevents silent
  loss of `workspace`, `restart_policy` and port allocations, and the round-trip
  test proves it.
- **`ProjectTopology` with `NonZeroU8` and derived cluster-ness.** Removes the
  `mode` / `node_count` contradiction by construction, and its serde path is
  genuinely safe — verified.
- **`LocalProject` / `CloudProject` split**, with the Cloud half deliberately
  absent from this repository.
- **`#[serde(transparent)]` for wire compatibility.** The reason M2 has zero
  compatibility impact — and, ironically, also the cause of F1. The fix is to
  keep the *serialized* shape and change only the *deserialization* path.

---

## Recommendation

**Hold M3.** Land a small "M2.1" that fixes F1 and F2 (blocking), plus F4–F7
(cheap, and each is a silent-corruption path), then the F3 identity backfill as
its own reviewable change. The model itself does not need redesigning — no field
should move except `record_count`, and that is a judgement call rather than a
defect.

---

# M2.1 outcome

**Landed:** 2026-08-08. Scope was the repair only — **M3 was not started, no
consumer was migrated, and no duplicate `Project` implementation was deleted.**

## F1 — validated newtype deserialization ✅ Fixed

`crates/valori-domain/src/validate.rs` (new) provides `validating_deserialize!`,
which implements `Deserialize` by deserializing the inner primitive and passing
it through the type's own `parse()`. Validation therefore lives in exactly one
place and cannot drift; no rule is restated inside a `Deserialize` impl.

Applied to `ProjectName`, `ModelId`, `SnapshotId` — every validated newtype in
the crate. `Serialize` is untouched and still `#[serde(transparent)]`, so the
emitted JSON is byte-identical; only *acceptance* changed.

The audit of "all other validated newtypes" found two that need nothing, and the
assumption is now asserted rather than believed:

| Type | Why it is already safe | Asserted by |
|---|---|---|
| `ProjectId` / `SessionId` / `InstallationId` | `Uuid`'s own `Deserialize` rejects malformed UUIDs | `uuid_ids_reject_malformed_input_through_deserialize` |
| `ProjectTopology` | `NonZeroU8`'s own `Deserialize` rejects `0` | `topology_rejects_zero_through_deserialize` |

`Timestamp` carries no invariant, so it needs no validation.

Verified rejected through `Deserialize`, at the type and composite level:
`../../etc/passwd`, `../../../tmp`, `/path`, `foo/bar`, `""`, `..`, `../..`,
`a/../../etc`, `a\b`, `C:\Windows`, `has space`, `has.dot`, `has:colon`,
`émoji`, a NUL-embedded name, `.`, `./relative` — plus non-string JSON
(numbers, `null`, arrays, objects), invalid `ModelId`s and empty `SnapshotId`s.

## F2 — project-name compatibility ✅ Fixed

`ProjectName::parse` now implements the **daemon's** contract, which is what
actually gated creation on disk: non-empty, ≤ 64 bytes, characters limited to
`[A-Za-z0-9_-]`. That character rule is what keeps the value safe as a directory
name — `/`, `\` and `.` remain unrepresentable, so no traversal was traded away
to gain compatibility.

The stricter UI rule became a **separate creation policy**,
`ProjectName::check_new_project_policy()`, returning
`DomainError::ProjectNamePolicy`. It constrains what may be created, never what
may be represented. Nothing is truncated, normalised or rewritten.

`_scratch`, `-tmp` and 64-character names are representable again, and
`new_project_policy_matches_the_typescript_validator` pins the creation policy
against a local reimplementation of `isValidName`, so the two cannot drift.

## F3 — project identity persistence ✅ Fixed

**How existing projects without ids are identified:** `ProjectManifest.id`
previously defaulted to `crate::new_id()`, which made "absent" indistinguishable
from "present" after deserialization. It now defaults to the **empty string**,
so absence is detectable.

**Migration strategy** (non-destructive, no separate migration step):
`JsonProjectStore::get()` calls a new `backfill_id()` which, when the id is
empty, mints one UUID v4 and immediately writes the manifest back. It is
idempotent — a manifest that already has an id is untouched. `create()` and
`import()` mint an id when the caller supplied none.

- The id is **random, never derived**. Not from the display name, not from the
  filesystem path — deriving from either would change identity on rename or
  move, which is exactly what this repair exists to prevent.
- **No existing id is ever reassigned.**
- If the manifest cannot be written (a project at rest may be `chflags
  uchg`-protected), the project keeps the freshly minted id for the process and
  a `tracing::warn!` is emitted. Returning an error would make such a project
  unlistable, which is worse — but the condition is never silent. This is the
  one residual risk, recorded below.

Tests: legacy manifest → first load → id persisted → second load → same id;
survives a fresh store instance (restart); written to disk, not held in memory;
existing ids never reassigned; identity independent of name and of store root;
preserved across a directory rename; preserved across a move to another root;
`list()` and `get()` agree.

## F4–F7 — silent-corruption paths ✅ All fixed

| # | Was | Now |
|---|---|---|
| **F4** | `manifest_from_domain` skipped the cluster block for a standalone topology, leaving a stale `replication: 3` so the write was silently discarded | Returns `Result`; cluster → standalone is `UnsupportedTopologyChange`. The manifest is left untouched. Clearing it instead would discard node port allocations, so the caller must decide explicitly. Promotion standalone → cluster still works. |
| **F5** | `ApiProject.is_cluster` was deserialized, could contradict `replicas`, and was silently ignored | `TryFrom<ApiProject>` rejects any payload where `is_cluster != (replicas > 1)` with `DomainError::InconsistentTopologyFlag` |
| **F6** | Both adapters saturated `dim` (`unwrap_or(u32::MAX)` / `unwrap_or(u16::MAX)`), silently rewriting a dimension that is immutable after first insert | Both return `DimensionOutOfRange`. The `u32 → usize` direction is checked too, so a 16-bit target would fail loudly rather than wrap. |
| **F7** | `index_from_domain` ended in `.unwrap_or_default()`, silently rewriting an unmatched variant to `Brute` | Exhaustive `match`. Adding a variant to either enum is now a **compile error**, which is the drift detection the old comment claimed a test provided. |

Both adapter entry points that mutate (`manifest_from_domain`,
`record_from_domain`) now return `Result<(), AdapterError>` rather than
defaulting silently.

**F12** was also fixed while adding the matrix: the substring-based
"never leaks a secret" assertion is superseded by structural checks at the
adapter boundary.

## Items deliberately not changed

| Item | Decision |
|---|---|
| `record_count` | **Not** added back to `Project`, per instruction. It remains excluded as a cached operational projection; documented as future status/metrics state. No status system was designed. |
| API field names | Untouched. `replication`, `createdAt` and every other public field are unchanged; DTO naming is M5's problem. |
| `apiKey` vs `api_key_ref` | Not implemented. Recorded as a security migration item below. No secret store was built. |
| F8 (`Project` derives serde) | Deferred — removing the derive would break the domain round-trip tests and is a design call, not a correctness fix. |
| F9, F10, F13 | Documented, not changed. |
| F11 (`Timestamp` public field), F14 (duplicate error type names) | Deferred as cosmetic. |

## Security migration item — credential storage divergence

Unchanged and still open, recorded here so it is not lost:

- `valori_daemon::EmbeddingConfig` stores `api_key_ref` — a **reference** to a
  credential. This is the correct shape.
- `ui/src/lib/server/projects.ts::ProjectEmbedConfig` stores `apiKey` — the
  **raw credential**, in a plaintext manifest on disk.

The canonical `Project` model deliberately carries neither, so M2.1 neither
worsened nor fixed this. **A secrets decision is a prerequisite for migrating
embedding configuration, not a follow-up to it.** Resolving the divergence by
lifting the TypeScript shape into a shared model would put a raw credential into
the platform contract, and must not happen.

## Tests added

| Suite | Before | After | Added |
|---|---|---|---|
| `valori-domain::invariants` (new) | — | 17 | **+17** |
| `valori-domain::project_contract` | 16 | 19 | +3 |
| `valori-domain::wire_compat` | 14 | 14 | 0 |
| `valori-daemon::domain_adapter` | 8 | 13 | +5 |
| `valori-daemon::project` (F3 identity) | 4 | 12 | **+8** |
| `valori-metadata::domain_adapter` | 6 | 10 | +4 |
| **Total** | | | **+37** |

`crates/valori-domain/tests/invariants.rs` is the required matrix: every type is
exercised through constructor, serialization, deserialization, invalid input,
the persistence boundary, and — where one exists — the adapter boundary. Adapter
coverage lives with the adapters because `valori-domain` cannot depend on
`valori-daemon` or `valori-metadata`.

Combined `valori-kernel` + `valori-node` + `valori-domain` + `valori-daemon` +
`valori-metadata`: **552 passing, 0 failing** (was 515).

## Compatibility impact

**Serialized output: unchanged.** `Serialize` was not modified for any type;
`ProjectName`, `ModelId` and `SnapshotId` still emit bare strings.

**Accepted input: narrowed, deliberately.** Values that were always invalid are
now rejected instead of silently admitted. No previously *valid* value was
rejected — `values_already_on_disk_still_deserialize` pins that.

**`project.json`: one additive change.** `id` now defaults to empty instead of a
fresh UUID, and is backfilled and persisted on first read. A manifest that
already has an id is byte-identical through a read. A manifest without one gains
the field it should always have had.

**Wider than before, not narrower, on names.** `ProjectName` now accepts a
superset of what it did at M2 — `_scratch`, `-tmp`, 64-character names.

**Rust API changes, all within M2 code no consumer calls yet:**
`manifest_from_domain` and `record_from_domain` now return `Result`.

**Untouched:** kernel formats, snapshot V6, event log V4, audit chain, redb
schema, HTTP handlers, Python SDK, FFI, cluster routing.

## Unresolved decisions

1. **Unwritable manifests.** If `project.json` cannot be written during id
   backfill, identity stays unstable for that project until it becomes writable.
   Warned, never silent. A `chflags uchg`-protected project at rest is the
   realistic trigger. Options: unlock-then-backfill during daemon start, or an
   explicit repair command.
2. **The name-rule split needs a UI decision.** The domain now accepts
   `_scratch`; the creation policy does not. Nothing calls
   `check_new_project_policy()` yet — M3 must wire it into the create path, or
   creation silently becomes more permissive than the UI implies.
3. **`dim == 0` is representable in `Project`.** The daemon rejects it at
   creation, the domain model does not. A `VectorDimension` newtype would close
   this and give the three-width mismatch one home (F6/question 12); deferred as
   a larger change than M2.1's scope.
4. **F8** — whether `Project` should be serializable at all.
5. **F13** — if per-collection index lands, `Project.index` becomes a default
   rather than a fact.
6. **Supabase schema** is still unknown, so `ApiProject`'s fit against the Cloud
   `projects` table remains unverified.
