# Valori Studio — redb Storage Architecture Audit

**Status:** Audit only. No code changed. No database created.
**Scope:** whether `redb` is a good fit for a Studio-level metadata/state
store, and whether existing `redb` infrastructure can be reused.

---

## 1. Existing redb usage — exact inventory

Two, and only two, crates depend on `redb` (`Cargo.toml` grep, workspace-wide):

| Crate | `redb` used for |
|---|---|
| `valori-consensus` | Persistent Raft log (`log_store_redb.rs`) |
| `valori-metadata` | Control-plane metadata (`db.rs`) |

`redb = "2"` in both `Cargo.toml`s; `Cargo.lock` pins `2.6.3`. No other crate,
and nothing under `desktop/` or `ui/`, references `redb`.

### 1.1 `valori-consensus::log_store_redb` — the Raft log

- **File:** [`crates/valori-consensus/src/log_store_redb.rs`](../../crates/valori-consensus/src/log_store_redb.rs)
- **What it is:** persistent backing store for one openraft `RaftLogStorage` —
  the write-ahead log, vote, and committed/purge pointers for **one Raft
  shard**.
- **Tables:** `logs: TableDefinition<u64, &[u8]>` (log index → bincode
  `Entry`), `meta: TableDefinition<&str, &[u8]>` (`vote` / `committed` /
  `last_purged` → bincode value).
- **Serialization:** `bincode::serde` (not JSON) — internal, not meant to be
  read by anything except this store.
- **Durability:** every write method commits its redb transaction before
  returning; redb commits are fsync-backed by default. The doc comment is
  explicit that this is load-bearing for Raft correctness (a lost vote can
  elect two leaders in the same term).
- **File path:** one redb file **per shard**, at `shard_path(base, shard_id,
  shard_count)` (`crates/valori-node/src/cluster.rs`), driven by
  `VALORI_RAFT_LOG_PATH`. Lives on the **node process's** disk, one file per
  Raft group. If `VALORI_RAFT_LOG_PATH` is unset the log is in-memory only
  (logged as a warning — "not crash safe").
- **Owner / process:** `valori-node` (cluster mode) / `valori-consensus`.
  Never opened by the daemon or by Studio.
- **Threading:** consumed through openraft's own actor model — one log store
  instance per shard, driven by the Raft core task. Not designed for
  arbitrary concurrent external access.

**Verdict:** this is Raft-internal infrastructure, not a general metadata
store. It is schema-locked to openraft's log/vote/purge concepts, keyed by
`u64` log index, and physically colocated with a specific node/shard's
lifetime. It has **no path** to Studio: Studio never runs a Raft shard.

### 1.2 `valori-metadata::db::MetadataDb` — control-plane metadata

- **File:** [`crates/valori-metadata/src/db.rs`](../../crates/valori-metadata/src/db.rs)
- **What it is:** "the single redb database that backs all control-plane
  metadata" (doc comment, `db.rs:29`). One `MetadataDb` per **Valori
  installation**, at `~/.valori/metadata.redb` (doc comment; the concrete
  `open()` call site is not yet wired into `main.rs` anywhere in the
  workspace — see §1.3).
- **Tables** (all `TableDefinition<&str, &[u8]>`, JSON-encoded values):

  | Table | Key | Value |
  |---|---|---|
  | `projects` | project name | `Project` (JSON) |
  | `collections` | `"project/collection"` | `Collection` (JSON) |
  | `planner_cache` | `PlannerCacheKey::to_db_key()` | `PlannerCacheEntry` (JSON) |

- **Transaction pattern:** every method opens its own `begin_write()` /
  `begin_read()`, does one table op, commits. No long-lived transactions, no
  transaction handles crossing an `await` point (the whole crate is sync —
  `valori-metadata` has no `tokio` dependency).
- **Concurrency:** `MetadataDb` wraps a bare `redb::Database`, no
  `Arc<Mutex<_>>`. It is passed around as `&MetadataDb` (e.g.
  `valori-planner/src/planner.rs:55`). This works because `redb::Database`
  is internally thread-safe (MVCC: one writer, many concurrent readers,
  enforced by redb itself) — callers don't need to add their own lock.
- **Owner:** `valori-planner` is the only in-tree **consumer** today (the
  durable half of its two-layer execution/planner cache — in-process
  `ExecutionCache` first, `MetadataDb::cache_get` on miss). `Project` and
  `Collection` CRUD methods exist and are unit-tested but have **no
  production call site** yet — every real project today is persisted by
  `valori-daemon`'s JSON `ProjectStore` (see §5), not `MetadataDb`.
- **Recovery / migration:** none implemented. `open()` just creates tables if
  absent; there is no schema-version table, no migration runner.
- **Test strategy:** `#[cfg(test)] mod tests` in `db.rs` — CRUD round-trips
  and cache expiry, each against a fresh `tempdir()` database. No crash/kill
  test, no concurrent-writer test.

### 1.3 Who actually opens `~/.valori/metadata.redb`

Grepping the whole workspace (excluding `target/` and test files) for
`MetadataDb::open` / `MetadataDb::new` / `metadata.redb` finds only:
the doc comment in `db.rs`, and the `tempdir()`-based test helper. **No
production code path in `valori-daemon`, `valori-node`, or `valori-cli`
currently calls `MetadataDb::open`.** The crate is real, tested, and
consumed by `valori-planner`'s cache logic — but the cache is constructed
with `db: Option<&MetadataDb>`, and every current call site passes `None`
(in-process cache only). This means:

- `crates/valori-metadata` is **built but not yet deployed** — it exists as
  a library, not yet as a running database anywhere in the product.
- There is no existing runtime lock, file, or process to conflict with.

### 1.4 Answering the ownership questions (§2 of the request)

**A. What is the existing redb database?**
There are two, and they answer differently:

1. `valori-consensus`'s Raft log → **internal to consensus** (openraft log
   storage). Not a general database at all.
2. `valori-metadata`'s `MetadataDb` → **a metadata database** — the closest
   existing thing to what Studio needs — but it is scoped to control-plane
   concepts (`Project`, `Collection`, planner cache) that belong to the
   **node/daemon boundary**, not to the desktop shell. It is not yet wired
   into any running process.

Neither is the actual **project data** database — vectors/graph/WAL/snapshots
live in `valori-kernel`'s own binary snapshot format + `events.log`, entirely
outside redb (see §5).

**B. Can Studio safely open that same database?**

- The Raft log: no. It is shard-scoped, keyed by Raft log index, driven by
  openraft's actor loop, and lives on the node process. Opening it from
  Studio would mean linking `valori-consensus` + `openraft` into the desktop
  binary for no reason, and risking a second process locking a file the
  node process expects to own exclusively (redb allows one writer at a
  time — a second `Database::open` from a different process is exactly the
  kind of cross-process contention `MetadataDb`'s own doc avoids by being
  single-owner).
- `MetadataDb`: technically closer, but wrong for three concrete reasons:
  1. **Table namespace is not Studio's** — `projects` / `collections` /
     `planner_cache` are control-plane concepts owned by `valori-domain` /
     `valori-metadata` per `docs/architecture/ownership.md`'s registry.
     Studio adding tables here would blur "which layer owns this concept."
  2. **Process ownership is undecided** — nothing currently opens this file
     at runtime. If a future daemon phase does open it (e.g. as `MetadataDb`
     becomes production-wired for `Project`/`Collection` persistence), a
     second process (Studio) opening the same file introduces exactly the
     cross-process writer contention redb's single-writer model doesn't
     eliminate on its own — SQLite-style WAL sharing isn't what redb
     provides; redb serializes writers within one `Database` handle in one
     process, not across processes on a shared file without external
     coordination.
  3. **Schema coupling** — a Studio table added to `metadata.redb` would
     require every future `valori-metadata` migration to also reason about
     Studio's tables, and vice versa. That is the coupling the request
     explicitly wants avoided.

**Conclusion: do not reuse either existing redb database.** Both are owned by
crates on the node/daemon side of the architecture, not the desktop shell.

---

## 2. Existing Studio persistence — full inventory

Search terms: `localStorage`, `sessionStorage`, `tauri-plugin-store`,
`preferences.json`, JSON files, SQLite, redb, cookies, IndexedDB (source
files only, `target/`, `node_modules/`, and `.next/` excluded).

### 2.1 Tauri (Rust) side — `desktop/src-tauri/`

| Mechanism | Where | Purpose |
|---|---|---|
| `tauri-plugin-store` (`preferences.json`, JSON file under the OS app-config dir) | registered in [`lib.rs:325`](../../desktop/src-tauri/src/lib.rs), read/written from [`telemetry.rs`](../../desktop/src-tauri/src/telemetry.rs) (`analytics_consent`, `installation_id`) and from the JS side via `native.ts` | The one Rust-native persisted file: telemetry consent + installation id, read without a JS round-trip |
| Flat JSON-lines file `events.jsonl` under `app_config_dir()` | [`telemetry.rs`](../../desktop/src-tauri/src/telemetry.rs) `queue_path`/`enqueue`/`drain_queue` | Durable **telemetry queue** — see §8 |
| Marker file `crashes/crash_marker.json` | `telemetry.rs` | One-shot crash marker written from a panic hook (no async/HTTP allowed mid-panic), read and cleared on next startup |
| `tauri-plugin-single-instance` | `lib.rs:320` | Enforces single OS-level app instance (answers §15) |
| No SQLite, no redb, no other embedded DB | (confirmed via `Cargo.toml` — only `tauri-plugin-store`, `reqwest`, `uuid`, `chrono` besides Tauri/OS plugins) | — |

### 2.2 JS/React side — `ui/`

| Mechanism | Where | Purpose |
|---|---|---|
| `tauri-plugin-store` via `ui/src/lib/native.ts`'s `getPreference`/`setPreference` (same `preferences.json` as §2.1) | `native.ts` | Onboarding version, recent/favorite/last-opened projects, last page, telemetry consent, installation id, daemon-related prefs |
| `localStorage` — raw key `valori:projects-list` | `ui/src/lib/hooks/useProjectManifest.ts` | "instant first paint" cache of the daemon's `/v1/projects` list — explicitly a **cache**, SWR is the source of truth |
| `localStorage` — embedding config (incl. `apiKey`) | `ui/src/lib/hooks/useEmbeddingConfig.ts` | Per-project(?) embed provider/model/endpoint/**apiKey** — see §11, this is a secrets finding |
| `localStorage` — LLM config (incl. `apiKey`) | `ui/src/lib/hooks/useLLMConfig.ts` | Same shape, for chat/LLM provider — same secrets finding |
| `localStorage` (Supabase's own session storage, keyed by Supabase's convention) + a mirrored cookie | `ui/src/utils/supabase/client.ts` | Cloud auth session (access/refresh tokens) — see §11 |
| `~/.valori/ui-projects.json` (legacy, filesystem, not browser storage) | referenced only as **migration source** in [`crates/valori-daemon/src/migration/m001_project_registry.rs`](../../crates/valori-daemon/src/migration/m001_project_registry.rs) — originally written by `ui/src/lib/server/projects.ts` | Legacy pre-daemon project registry; migrated into per-project `project.json` once, then renamed to `.migrated` |

No IndexedDB, no `sessionStorage`, no SQLite anywhere in `ui/` or
`desktop/src-tauri/`.

### 2.3 Data table (§4 of the request)

| Data | Current storage | Owner | Lifetime | Candidate for Studio redb? |
|---|---|---|---|---|
| Onboarding completion version | `preferences.json` (tauri-plugin-store) | `native.ts` | Until app reinstall/reset | Yes — authoritative, tiny |
| Telemetry consent (`analytics`, `crash`) | `preferences.json` | `native.ts` + `telemetry.rs` (reads it directly, Rust-native events) | Until user changes it | Yes, but **only if** Rust-native readers move too (§2.1 shows Rust reads this file directly today — moving it to redb means updating `telemetry.rs`, not just JS) |
| Installation id | `preferences.json` | `native.ts` + `telemetry.rs` | Permanent (per install) | Yes |
| Recent projects (≤8), favorite projects, last-opened project, last page | `preferences.json` | `native.ts` | Rolling / permanent | Yes — this is exactly "local application state" the request calls out |
| Projects list cache (`valori:projects-list`) | `localStorage` | `useProjectManifest.ts` | Cache only, SWR-refreshed | Yes, as an explicit **cache** table, not authoritative |
| Embedding config incl. `apiKey` | `localStorage` | `useEmbeddingConfig.ts` | Until cleared | **No** — not without a secrets decision first (§11); plaintext in either store is the same exposure, moving it to redb doesn't fix it |
| LLM config incl. `apiKey` | `localStorage` | `useLLMConfig.ts` | Until cleared | Same as above — **no** |
| Cloud auth session (Supabase tokens) | `localStorage` + cookie | `supabase/client.ts` | Session-lived, Supabase-managed refresh | **No** — Supabase's SDK owns this storage contract; redirecting it to redb is a Supabase-integration change, out of scope and not requested |
| Telemetry event queue | `events.jsonl` | `telemetry.rs` | Until delivered, capped at 500 lines | Yes — this is the strongest single case for redb, see §8 |
| Crash marker | `crashes/crash_marker.json` | `telemetry.rs` | One-shot, cleared next launch | Marginal — see §8 (panic-safety argument cuts the other way) |
| Legacy `ui-projects.json` | filesystem JSON | `m001_project_registry.rs` (migration only) | Renamed to `.migrated` after one-time import | No — dead once migrated, not a live data source |

---

## 3. Project storage boundary — Studio metadata vs. Valori project data

This boundary already has a name and an owner in the codebase: it is
documented in [`docs/architecture/ownership.md`](ownership.md) (untracked in
this branch, already written) and enforced by
[`crates/valori-node/tests/dependency_direction.rs`](../../crates/valori-node/tests/dependency_direction.rs).

```text
Studio (desktop shell)
  │  preferences.json / events.jsonl / localStorage  ← Studio-local state
  ▼
valori-daemon  (Runtime, ProjectStore trait, JsonProjectStore impl)
  │  ~/.valori/projects/<name>/project.json           ← the manifest
  ▼
Valori project storage  (per-project directory, owned by valori-node/valori-kernel)
  ├── events.log        (valori-wire V4, WAL/audit chain)
  ├── snapshot.val       (valori-kernel V6 snapshot — vectors, graph, indexes)
  └── (cluster only) raft-shardN.redb  (valori-consensus, per shard)
```

Concretely, from source:

- **Project manifest**: `crates/valori-daemon/src/project.rs`'s
  `ProjectManifest` — plain JSON, one file per project
  (`~/.valori/projects/<name>/project.json`), read/written through the
  `ProjectStore` trait (`crates/valori-daemon/src/store.rs`), implemented
  today only by `JsonProjectStore`. The trait doc comment explicitly names
  `SqliteProjectStore` / `CloudProjectStore` as *future, not current*
  alternate implementations — daemon code depends on the trait, not the
  filesystem.
- **Vectors, graph, indexes**: `valori-kernel`'s binary snapshot format (V6),
  written by `valori-kernel/src/snapshot/encode.rs`, one file per project
  (`snapshot.val`/`current.snap` depending on path). Never touches redb.
- **WAL / audit chain**: `events.log`, `valori-wire` V4 format, append-only,
  BLAKE3-chained. Never touches redb.
- **Cluster Raft log** (only when a project runs in cluster mode): one redb
  file **per shard** via `shard_path()` in `crates/valori-node/src/cluster.rs`,
  owned by `valori-consensus`, living inside that project's node data
  directory — not `~/.valori/metadata.redb`, not anything Studio-level.
- **Control-plane metadata** (`valori-metadata::MetadataDb`): scoped to
  `Project`/`Collection`/planner-cache — a **node/daemon-side** control
  plane concept per `ownership.md`'s registry row ("Operation / Planner /
  ExecutionGraph / Executor" → `MetadataDb` execution + planner cache). Not
  Studio's.

**What must never land in `studio.redb`:** vectors, documents, WAL, snapshots,
indexes, graph data, embeddings (the vector data itself — not the *config*
naming a provider), model artifact bytes, project manifests, Raft log
entries, planner cache, collection→namespace mappings. All of these already
have a defined owner outside the desktop shell, and duplicating them would
create exactly the "four representations of one concept" problem
`ownership.md` was written to stop.

**What legitimately is Studio's own concern:** Studio-local UI/session state
that describes *how the desktop app remembers itself* — which project was
open, which were favorited, whether onboarding finished, whether telemetry
is consented, and (cache only, never authoritative) a locally-mirrored
summary of what the daemon last reported. All of this is at or below the
`Project` domain type in the ownership hierarchy — Studio never invents new
project *meaning*, only local-app *memory* of it.

---

## 4. Candidate Studio redb schema

Do not implement. Proposed logical tables, each justified individually.

| Table | Exists today (as)? | Owner | Key | Value | Retention | Indexed? | Secrets? | Local-only? | Rebuildable? | Migration/versioning needed? |
|---|---|---|---|---|---|---|---|---|---|---|
| `preferences` | `preferences.json` (tauri-plugin-store) | Studio | `&str` (pref name) | JSON scalar/object | Until user changes | No | No | Yes | No (it's the source of truth for user choices) | Yes — new prefs get defaults, no destructive change needed |
| `recent_projects` | `preferences.json` key `recentProjects` | Studio | ordinal (or single JSON array value under one key, matching current shape) | project name list, capped at 8 | Rolling window | No | No | Yes | Yes (rebuildable from daemon's project list, order is UX-only) | No |
| `favorite_projects` | `preferences.json` key `favoriteProjects` | Studio | project name (set semantics) | boolean/presence | Until user unfavorites | No | No | Yes | No — this is a user *choice*, not derivable from the daemon | No |
| `project_cache` | `localStorage` key `valori:projects-list` | Studio | project name | last-known `ManifestProject` JSON | Until next successful `/v1/projects` fetch | No | No | Yes | **Yes, trivially** — refetched from daemon every load | No |
| `sessions` | *(not persisted today — see §9)* | Studio | session id (uuid, matches `telemetry.rs`'s `SESSION_ID`) | start time, app version, platform | Duration of process | No | No | Yes | Yes (informational only) | No |
| `telemetry_queue` | `events.jsonl` flat file | Studio (emitter), Cloud (sink) | event id (uuid) | full envelope (schema, session_id, installation_id, event, properties, timestamp) | Until delivered; capped | No (append + drain, no lookups needed) | Only if a `properties` payload ever carries one by mistake (see §11) | Yes until sent | Yes (loses only undelivered events) | Envelope already carries `schema: u32` — carry it into the table too |
| `sync_state` | *(not implemented)* | Studio (cache half), Cloud (authoritative half) | key = concept name (e.g. `"cloud_projects"`) | last-sync cursor/timestamp | Until next sync | No | No | Yes | Yes | No |
| `cloud_project_refs` | *(not implemented — see §10)* | Studio (reference), Cloud (authoritative) | `project_id` (shared with local `ProjectId`, per `ownership.md`) | `{ organization_id, cloud_endpoint, last_sync, cached_display_name }` | Until unlinked | Maybe, by `organization_id` if multi-org matters later | No (references, not credentials) | No — mirrors Cloud state | Yes (re-fetchable from Cloud) | Yes |
| `updates` | *(not persisted — updater state is in-memory + OS updater plugin)* | Studio | singleton key | last-checked timestamp, last-known version, install outcome | Until superseded | No | No | Yes | Yes | No |
| `models` | *(not in Studio at all today — `~/.valori/models` is a filesystem dir referenced by `valori-node`, not Studio)* | **Not Studio's** — leave with `valori-node`/model manager | — | — | — | — | — | — | — | — |

Explicitly **not** proposed, per the request's own caution against blindly
creating every suggested table:

- `executions` — see §9: pipeline/planner executions belong to
  `valori-metadata`'s `ExecutionRecord`, not Studio. Studio has no local
  concept of "an execution" independent of a project.
- `models` table inside `studio.redb` — model artifacts and their metadata
  already have an owner (`valori-models` / `~/.valori/models`); Studio only
  ever *displays* that state, it doesn't need its own copy.
- A dedicated `apiKey`/`credentials` table — explicitly out of scope until a
  secrets decision is made (§11); do not backfill a redb table with the same
  plaintext-secret problem `localStorage` already has.

---

## 5. Authoritative vs. derived vs. cache vs. queue vs. ephemeral

| Record | Classification | Why |
|---|---|---|
| `ProjectId` | **Authoritative** (in `valori-domain`, not Studio) | Studio never mints one; it only references IDs the daemon/Cloud assigned |
| Project path (`dir`) | **Authoritative**, but owned by daemon's `project.json`, not Studio | Studio's `project_cache` row holding a `dir` value is a **cache** mirror of it |
| Cloud project ID | **Authoritative in Cloud** | Studio's `cloud_project_refs.project_id` is a **cache/reference**, never edited locally |
| Last opened time | **Authoritative in Studio** — this genuinely only exists as local UX memory | Nothing else tracks "when did *this desktop install* last open project X" |
| Telemetry event | **Queue**, becomes **ephemeral** once delivered | It is data-in-transit, not state Studio reasons about after delivery |
| Sync cursor | **Cache** (mirrors Cloud's notion of "what's been synced") | Must never be promoted to authoritative — if Studio's cursor and Cloud's actual state diverge, Cloud wins by definition |
| Model metadata | **Cache**, if Studio ever stores it — authoritative copy is the model manager / model provider | Do not duplicate into `studio.redb` at all per §4 |
| Update state | **Authoritative for "what did *this install* last check/do"**, but the update itself (available version, changelog) is **derived** from the update server each check | Storing "last checked at T, found version V" is fine; never treat a cached "latest version" as authoritative for update-availability without re-checking |
| Session | **Ephemeral** | Scoped to one process lifetime; nothing needs a session to outlive the app, other than the telemetry envelope that already captured it |

The one sharp edge worth calling out: **`project_cache`** (today's
`localStorage` "instant first paint" cache) must stay literally read-only
from Studio's perspective — the existing code already treats it this way
(SWR fetch is the write path, the cache read is only for first paint before
SWR resolves). A redb version must preserve that discipline: no code path
should ever answer "does project X exist" from `project_cache` alone.

---

## 6. Telemetry queue — is redb appropriate?

Current implementation (`desktop/src-tauri/src/telemetry.rs`): a flat
JSON-lines file (`events.jsonl`), guarded by a single `Mutex<()>`
(`QUEUE_LOCK`) around every read-modify-write, capped at `MAX_QUEUE_LINES =
500` (oldest dropped), drained on a timer by `spawn_sender` /
`drain_queue`: read whole file → POST each line → rewrite file with only
the still-failed lines (or delete it if all succeeded).

Required fields, cross-checked against the existing `TelemetryEnvelope`
struct: `event_id` (uuid) ✅, `session_id` ✅, `installation_id` ✅, `schema`
(version) ✅ — but note **no `retry_count` field exists today**; a failed
send is just kept verbatim and retried next tick, forever, with no backoff
and no give-up threshold. `created_timestamp` ✅ (`timestamp`, RFC3339).
`payload` ✅ (`properties: serde_json::Value`). `delivery_state` is implicit
(presence in the file = undelivered) rather than an explicit field.

**Is redb appropriate for this queue?** Yes, better than the current flat
file, for one concrete reason: the current implementation reads the
**entire** file, rewrites the **entire** file, on every drain tick and every
enqueue — an O(n) rewrite under a single mutex for what is conceptually a
per-event append/delete workload. At the current cap (500 lines of small
JSON) this is not a real performance problem, but it is exactly the
head-of-line, single-file-rewrite pattern append-only + random-delete
databases exist to avoid. A `telemetry_queue` redb table keyed by
`event_id` would let `enqueue` insert one record and `drain_queue` delete
delivered records individually, no longer needing to rewrite everything
that didn't move.

**Caveats before recommending it as a live change (not requested — audit
only):**

- The panic-hook path (`CRASH_MARKER_FILE`) is deliberately **not** async
  and deliberately **not** the telemetry queue — the doc comment explains
  redb transactions inside a panic handler would carry the same soundness
  risk as the async HTTP call it's avoiding. **Do not route the crash
  marker through redb** — keep it as the plain marker file it is; only the
  steady-state event queue (`enqueue`/`drain_queue`, both called from normal
  async context) is a redb candidate.
- `MAX_QUEUE_LINES` (500) already caps growth; a redb-backed queue needs the
  equivalent cap enforced on insert (oldest-first eviction), not left
  implicit.
- Add the missing `retry_count` if this migrates — the current
  keep-forever-and-retry behavior has no bound today regardless of storage
  engine; that is a pre-existing gap the audit surfaces but does not fix.

---

## 7. Sessions and execution history — three concepts, not one

The request is right to flag this: there are three genuinely different
things that all "have timestamps and statuses," and merging them would be a
mistake.

1. **Studio application session** — "this desktop process ran from T1 to
   T2." Exists today only as `telemetry.rs`'s in-memory `SESSION_ID`
   (`OnceLock<String>`), never persisted as its own record — it's stamped
   onto telemetry envelopes, not stored standalone. **Belongs in
   `studio.redb`** if persisted at all (it's purely local-app bookkeeping).
2. **Valori execution** (planner/effect-system operation run) —
   `valori-planner`'s `ExecutionStatus` + `valori-metadata`'s
   `ExecutionRecord` (per `ownership.md`'s registry: "Execution status /
   history" → owner `valori-planner`/`valori-metadata`, persisted in
   **metadata redb**, i.e. `MetadataDb`, not Studio). This is a per-project,
   server-side concept — it exists whether or not Studio is even running,
   and multiple UIs (Studio, Cloud dashboard, CLI) could observe the same
   execution. **Does not belong in `studio.redb`.**
3. **Pipeline execution** — not a distinct persisted concept found in this
   codebase; the request's phrasing likely refers to the same `Operation` /
   `ExecutionGraph` machinery as (2) (`valori-planner` + `valori-effect`,
   per `ownership.md`'s "Operation / Planner / ExecutionGraph / Executor"
   row). Same owner, same answer: not Studio's.

**Recommendation:** a `sessions` table in `studio.redb` is legitimate and
small (start time, app version, platform — matching `AppInfo` already
returned by `get_app_info`), but it must not be conflated with or query
`ExecutionRecord`. If Studio ever wants to show "recent executions," it
should read them from the node/daemon's execution API, not duplicate them
locally.

---

## 8. Cloud projects — local representation

Confirmed from `crates/valori-domain/src/project.rs`'s doc comment
(`project.rs:44-56`, `:479-486`): the domain deliberately excludes
`organization_id`, `region`, `deployment_id` from the shared `Project` type
— "a local project has no organization" — and names `CloudProject` as the
type that composes `Project` + those fields, living in the **private**
Cloud repository, correlated only by the shared `project.id`
(`ProjectId`).

Today, `ui/src/app/cloud/CloudProjectsClient.tsx` defines its own
`CloudProject` TS type and Cloud pages read directly from Supabase
(`org_id`-scoped queries) — there is **no local cache of Cloud project
metadata in Studio today**; Cloud pages are server-rendered/fetched live,
not mirrored into desktop-local storage. `desktop/src-tauri/src/lib.rs`'s
`open_cloud_login` / the `auth-callback` deep-link handler explicitly do
**not** touch project data — only auth tokens pass through, and even those
go straight to the embedded webview's Supabase client, never stored or read
on the Rust side (see the code comment at `lib.rs:405-411`).

**What Studio should store, if/when it needs an offline-aware reference to
a Cloud project** (not implemented — proposed only): exactly the shape the
request suggests —

```text
cloud_project_refs[project_id] = {
    organization_id,
    cloud_endpoint,
    last_sync: timestamp,
    cached_display_name: string,   // for offline project-switcher rendering
}
```

— never the full `CloudProject` object, never credentials. Cloud remains
authoritative; this is purely a "what can Studio show before the network
round-trip completes" cache, in the same spirit as `project_cache` in §4/§5.

---

## 9. Secrets audit

| Location | What | Classification |
|---|---|---|
| `ui/src/lib/hooks/useEmbeddingConfig.ts` → `localStorage` | Embedding provider `apiKey`, plaintext | **Unsafe** |
| `ui/src/lib/hooks/useLLMConfig.ts` → `localStorage` | LLM provider `apiKey`, plaintext | **Unsafe** |
| `ui/src/components/settings/SettingsModal.tsx` (`rerankerKey` state, `type="password"` input) | Feeds the same `apiKey` fields above | **Unsafe** (the `type="password"` input masks display only; storage is still plaintext `localStorage`) |
| `ui/src/lib/hooks/useProjectManifest.ts`'s `ManifestProject.embed?.apiKey` | The daemon's own manifest type **carries an optional `apiKey` field** in its TS shape | **Needs migration / unknown** — `valori-domain/src/project.rs`'s doc comment explicitly calls this out: *"The TS shape carries an `apiKey`. A shared model that can hold a secret needs a secrets decision first"* — i.e., the project team already identified this gap and deliberately deferred it rather than resolving it. Whether the daemon's `project.json` on disk actually persists this key in plaintext depends on `EmbeddingConfig`/`api_key_ref` in `crates/valori-daemon/src/project.rs` — that struct stores `api_key_ref: Option<String>` (a *reference*, e.g. an env var name), explicitly **not** the raw secret, per its own doc comment ("never the raw secret... unlike ui/'s current `ProjectEntry.embed.apiKey`"). So: **daemon-side is safe by design; UI-side (`localStorage`) is not**, and the two are inconsistent with each other today. |
| Supabase auth tokens (`ui/src/utils/supabase/client.ts`) | access/refresh tokens in `localStorage` + a mirrored cookie | **Standard practice for Supabase's SSR client**, not a Valori-specific gap — classified **safe** under "this is the documented pattern for the library in use," though it is still plaintext-in-browser-storage by nature of that library |
| `desktop/src-tauri/src/lib.rs` auth-callback handler | Explicitly does **not** read or store tokens on the Rust side (comment confirms this) | **Safe** |
| `crates/valori-daemon/src/project.rs::EmbeddingConfig.api_key_ref` | Reference only, never raw secret, by design | **Safe** |
| `telemetry.rs` envelopes | `properties: serde_json::Value` — arbitrary JSON from call sites | **Unknown** — nothing in the reviewed call sites passes a secret today, but the type itself does not prevent it. Worth a lint/convention, not a storage-engine fix. |

**Bottom line:** the two `localStorage` `apiKey` fields are a real,
pre-existing plaintext-secret exposure, **independent of whether Studio
adopts redb**. Moving them into `studio.redb` without encryption-at-rest
would not fix this — it would only change which plaintext file holds the
key. Do not treat "put it in redb" as a secrets fix; that needs an actual
secret-store decision (OS keychain via a Tauri secret-store plugin, or
similar) which is explicitly out of scope here per the request's own
instruction not to design one yet.

---

## 10. Concurrency

Studio's Rust side (`desktop/src-tauri/src/`, 5 files: `lib.rs`,
`daemon_manager.rs`, `ui_server_manager.rs`, `telemetry.rs`, `main.rs`) is a
single Tauri process running:

- Tauri's own command dispatch (one async task per invoked `#[tauri::command]`, on the tokio runtime Tauri manages)
- `daemon_manager.rs` — supervises the `valori-daemon` child process
- `ui_server_manager.rs` — supervises the bundled Next.js server (release builds only)
- `telemetry.rs`'s `spawn_sender` — one long-lived background task, ticking on a timer, calling `drain_queue`
- The auto-updater (background check spawned ~8s after startup, per `lib.rs` comments)
- A filesystem-watcher was **not found** in the reviewed files — no `notify`-crate usage in `desktop/src-tauri/Cargo.toml`

Multiple of these **can** run concurrently: a `#[tauri::command]` call
(triggered by a JS button click) can fire while the telemetry sender's timer
tick is mid-drain, while the updater's background check is also running.
`telemetry.rs` already demonstrates the correct pattern for this today: a
single `static QUEUE_LOCK: Mutex<()>` guarding the one shared resource
(`events.jsonl`), held only across the read-modify-write, not across the
network call.

**Recommended ownership pattern for `StudioDatabase`, if built:** do **not**
wrap it in `Arc<Mutex<redb::Database>>`. `MetadataDb`'s existing pattern
(§1.2) — a bare `Database` behind `&self` methods, no external mutex — is
the right model, because redb's `Database` already serializes writers
internally (one write transaction at a time) and allows concurrent readers
without any caller-side locking. Wrapping it in an `Arc<Mutex<_>>` on top
would only add unnecessary contention (every reader would block on a mutex
that redb's own transaction model doesn't require), and would risk
holding the mutex across an `.await` if a callsite got it wrong. The correct
shape is `Arc<StudioDatabase>` (cheap to share across Tauri's `State<T>`
extractor and the background telemetry task) wrapping a plain
`redb::Database`, exactly mirroring `MetadataDb`.

---

## 11. Crash safety and durability

redb 2.6.3 (pinned in `Cargo.lock`), same version already vetted for the
Raft log — where the project's own doc comment states the exact guarantee
being relied on: **"redb commits are fsync-backed by default,"** and that
guarantee is load-bearing for Raft correctness (a lost vote after
acknowledgment can elect two leaders). That is a stronger durability bar
than Studio metadata needs, which is a point in redb's favor, not against
it — Studio inherits a durability guarantee already proven correct enough
for consensus-critical state.

- **Crash during write:** redb's transaction model means a crash mid-write
  either loses the uncommitted transaction entirely (never partially
  applies it) or — for the specific write that already returned from
  `commit()` — is durable per the fsync guarantee above. This is materially
  safer than the current `events.jsonl` telemetry queue, which does a
  non-atomic `fs::write` (read-modify-rewrite) that **could** leave a
  truncated/corrupt file if the process is killed mid-write — a genuine gap
  in the current implementation redb would close.
- **Application kill / OS shutdown:** same analysis — no open write
  transaction survives, and no committed one is lost, contingent on the
  underlying filesystem honoring fsync (true for all Studio's supported
  platforms' default filesystems).
- **Concurrent read/write:** redb's MVCC model means readers never block on
  a writer and see a consistent snapshot — no special handling needed
  beyond §10's guidance.
- **Partially written state / corruption:** not independently verified in
  this audit (no fault-injection test exists for either existing redb user
  — `MetadataDb`'s test suite is CRUD-only, no crash-simulation test).
  This is a real gap, but it is a gap in **redb's proof within this
  codebase**, not evidence against redb generically — the crate's own
  design docs (external to this repo) claim ACID transactions; nothing here
  contradicts that claim, but nothing here proves it under fault injection
  either. Treat this as an open validation item if Studio adopts redb (a
  `dr_disaster_recovery`-style test, mirrored from `valori-node`'s existing
  one referenced in `CLAUDE.md`, would close it).
- **Recovery / backup / rebuild:** every proposed table in §4 was
  independently classified for rebuildability; the only genuinely
  non-rebuildable data is user *choices* (`preferences`, `favorite_projects`)
  and Studio-native history (`sessions`, if kept). Everything else can be
  reconstructed from the daemon or Cloud, which materially lowers the
  stakes of a corrupted `studio.redb` — worst case is "forgot your
  favorites and recents," not data loss.

---

## 12. Schema migration

**Today:** neither existing redb user (`log_store_redb.rs`,
`valori-metadata/db.rs`) has a schema-version table or a migration runner.
`MetadataDb::open` just calls `open_table` for each known table — additive
only (a new table can be added freely; nothing currently handles renaming
or restructuring an existing table's key/value shape). The closest thing to
a migration system anywhere in the reviewed code is
`crates/valori-daemon/src/migration/` — but that's a **filesystem/JSON**
migration runner (the `m001_project_registry` one-time import), not
redb-aware, and it deliberately never touches redb.

**Smallest safe mechanism for `studio.redb`, if built** (proposed, not
implemented): a single `meta` table (mirroring `log_store_redb.rs`'s own
`META` table pattern — precedent already exists in this codebase) holding
one key, `"schema_version": u32`. `StudioDatabase::open()` reads it (0/absent
= fresh database, write `CURRENT_VERSION` and return), compares to a
compile-time constant, and if `stored < CURRENT_VERSION`, runs an ordered
list of migration closures `fn(&WriteTransaction) -> Result<()>` before
committing and bumping the version — additive, table-by-table, never a
blind drop-and-recreate. If `stored > CURRENT_VERSION` (a downgrade), fail
loudly rather than silently truncating unfamiliar data, mirroring
`native.ts`'s own stated policy for `ONBOARDING_VERSION` ("someone on a
newer version than this build expects is left alone").

---

## 13. Multi-instance Studio

`tauri-plugin-single-instance` is already a dependency
(`desktop/src-tauri/Cargo.toml:31`) and is **already wired up**
(`lib.rs:320`, `tauri_plugin_single_instance::init(...)`). Launching a
second `Valori Studio` today does not produce a second process with its own
state — the plugin's standard behavior (focus the existing window / forward
args) applies. This means:

- **No new single-instance mechanism is needed** — one already exists and
  already covers the redb-file-locking concern by construction: only one
  Studio process ever has `studio.redb` open at a time.
- The multi-*process* question the request raises (§15) is therefore
  already answered at the application level, before it becomes a database
  concern. redb's own single-writer-per-`Database`-handle model is a good
  match for "exactly one process, one open handle" — which is exactly what
  single-instance already guarantees.

---

## 14. Performance

Estimated actual workload, from the inventory above — not hypothetical:

- **Reads/writes:** dozens of preference reads on startup, single-digit
  writes on user actions (favorite a project, change a setting), one
  telemetry-queue write per tracked event (bounded by product surface, not
  by data volume), one queue-drain pass per sender tick (default interval
  not found in the reviewed slice of `lib.rs` — presumably minutes-scale
  given the "flushes anything queued... then every `interval`" doc comment).
- **Data volume:** every proposed table's values are small JSON objects —
  preferences, project names, short-lived telemetry envelopes. No table in
  §4 is expected to exceed low thousands of rows (recent/favorite projects
  are explicitly capped; telemetry queue caps at 500 today).
- **Fit:** this is squarely inside redb's sweet spot — an embedded,
  single-process, low-write-volume KV store with small values. It is a good
  fit for exactly the reason `MetadataDb` already chose it: no server
  process, no separate dependency to install, ACID transactions "for free."
- **Not a fit for:** none of the proposed Studio tables come close to
  needing what vector/document/analytics/log workloads need (the request's
  own list of things it should NOT become). Confirming the negative: no
  table in §4 has an unbounded growth path except `telemetry_queue`, which
  is already capped by the existing 500-line policy this audit carries
  forward.

---

## 15. Reuse options — decision

Evaluating the four options against the actual architecture found above:

- **Option A (reuse existing redb crate directly, no new database):**
  Rejected — §1.4 already establishes neither existing redb file should be
  shared with Studio. "Reuse the crate dependency, not the database file" is
  actually what's being recommended (see below) — Option A as literally
  stated (share the *database*) is wrong; as a dependency choice
  (`redb` the crate) it's simply correct and not really a distinct option.
- **Option B (dedicated `valori-studio-storage` crate):** This is the
  option that matches the codebase's own established pattern. `valori-
  metadata` already exists as *exactly this kind of crate* — a small,
  focused, redb-backed persistence crate with one typed API surface
  (`MetadataDb`) — for the control-plane layer. A `valori-studio-storage`
  crate mirroring that shape for the desktop-shell layer is consistent,
  not novel. It also gives Studio a Rust crate the daemon/node side never
  needs to depend on (desktop-only concerns like telemetry queue shape,
  UI-state schema), keeping the dependency graph clean per `layers.md`'s
  constitution — `valori-daemon`/`valori-node` must never depend on
  something that only exists for the desktop shell.
- **Option C (extend an existing storage crate with an isolated namespace):**
  Rejected — extending `valori-metadata` would violate the very ownership
  boundary `ownership.md` exists to enforce (Studio state is not a
  control-plane concept), and extending `valori-daemon` would pull desktop-
  UI concerns into the crate that `valori-node`/`valori-cli` also depend on.
- **Option D (keep existing redb DBs, add a separate Studio redb database):**
  This is functionally what Option B produces, minus the crate boundary.
  Given the codebase's existing convention (one small crate per persistence
  concern: `valori-storage`, `valori-metadata`, now `valori-studio-storage`)
  and given Studio's Rust code already lives in its own package
  (`desktop/src-tauri`, a separate Cargo workspace member per its own
  `Cargo.toml`), putting `StudioDatabase` code in a proper crate rather than
  directly in `desktop/src-tauri/src/` is the better fit — it's testable
  independent of Tauri, and the crate layout table in `CLAUDE.md` already
  reserves this kind of slot ("New Python SDK method" / "New env var" rows
  show the project's habit of one crate = one concern).

**Recommendation: Option B — a new `valori-studio-storage` crate**, opening
its own `~/.valori/studio.redb`, separate file from both existing redb
databases. It should depend on `redb` and, where genuinely useful,
`valori-domain` (for `ProjectId`/`InstallationId`/`SessionId` types — per
`ownership.md`'s registry, these value types are meant to be reused
transparently across boundaries) — but **not** on `valori-metadata`,
`valori-daemon`, or `valori-consensus`. `desktop/src-tauri` depends on
`valori-studio-storage`, the same way it will eventually depend on other
platform crates.

---

## 16. Proposed Rust API (not implemented)

```rust
// crates/valori-studio-storage/src/lib.rs  — PROPOSED, NOT IMPLEMENTED

pub struct StudioDatabase {
    db: redb::Database,   // no external Arc<Mutex<_>> — see §10
}

impl StudioDatabase {
    /// Opens (or creates) `~/.valori/studio.redb`. Runs pending migrations
    /// (§14) before returning. Fails loudly on a downgrade (stored version
    /// newer than this build's CURRENT_VERSION).
    pub fn open(path: &std::path::Path) -> Result<Self, StudioStorageError>;

    // One typed accessor per table, mirroring MetadataDb's per-concept
    // methods rather than exposing raw redb::Table handles — this is what
    // keeps "arbitrary tables accessed everywhere" from happening, per the
    // request's own requirement in §19.

    pub fn preferences(&self) -> PreferencesHandle<'_>;
    pub fn recent_projects(&self) -> RecentProjectsHandle<'_>;
    pub fn project_cache(&self) -> ProjectCacheHandle<'_>;
    pub fn telemetry_queue(&self) -> TelemetryQueueHandle<'_>;
    pub fn cloud_project_refs(&self) -> CloudProjectRefsHandle<'_>;
    pub fn sessions(&self) -> SessionsHandle<'_>;
    // sync_state / updates folded into `preferences` as singleton keys
    // unless/until they grow enough fields to warrant their own table —
    // avoid speculative tables per CLAUDE.md's "Simplicity First".
}
```

Each `*Handle` is a thin, single-table wrapper (own `get`/`set`/`list`
methods scoped to exactly that table's key/value types) constructed
on-demand from `&StudioDatabase`, not stored — matching `MetadataDb`'s
existing per-method-transaction pattern (§1.2), not a long-lived
transaction handle.

---

## 17. Migration plan (sequencing only — not executed)

If this audit's recommendation is accepted, the smallest safe order is:

1. Create `crates/valori-studio-storage` with `StudioDatabase` +
   `preferences`/`recent_projects`/`project_cache` tables only — the three
   pieces of state that are already unambiguously Studio's own and already
   have a single existing storage location (`preferences.json`) to migrate
   from, one-for-one.
2. Wire `desktop/src-tauri` to open `StudioDatabase` at startup, write-
   through both stores for one release (read from redb, fall back to
   `tauri-plugin-store` if absent) to avoid a hard cutover, matching the
   caution already shown in `m001_project_registry.rs`'s "never deletes the
   source" policy.
3. Migrate `telemetry_queue` next (§6) — highest-value move, but touches
   the async sender loop, so sequence it after the simpler tables prove the
   crate works.
4. `sessions` and `cloud_project_refs` last — both are net-new persisted
   concepts (§7, §10), not migrations of existing state, so they carry no
   compatibility risk and can wait until there's an actual consumer.
5. Never migrate the `apiKey` fields (§11) into `studio.redb` as part of
   this plan — that requires the separate secrets decision the request
   explicitly deferred.

---

## 18. Risks and open decisions

- **Secrets:** the plaintext `apiKey` exposure in `localStorage` (§11) is
  real and pre-existing; adopting redb does not fix it and should not be
  presented as fixing it.
- **`MetadataDb` is unwired:** if a future phase wires
  `valori-daemon`/`valori-node` to actually open `~/.valori/metadata.redb`
  in production, re-confirm at that time that `studio.redb` and
  `metadata.redb` remain genuinely separate files/processes — this audit's
  "no conflict" conclusion (§1.4) currently rests partly on `MetadataDb`
  having no live opener yet.
- **Telemetry retry_count gap** (§6): pre-existing, orthogonal to the
  storage engine, worth flagging to whoever owns telemetry next.
  Do NOT auto-fix as part of any redb migration — scope creep.
- **No crash/fault-injection test exists** for either current redb user
  (§13) — an open validation item before treating durability as fully
  proven within this codebase's own test suite, independent of upstream
  redb's own guarantees.
- **`sessions`/`cloud_project_refs` have no current consumer** — building
  them ahead of a UI that reads them would violate CLAUDE.md's "no
  speculative...configurability that wasn't requested" guidance; sequence
  them last (§17) precisely because they're not blocking anything today.
- **Rust-side telemetry consent read** (`telemetry.rs`'s `analytics_consent`
  reads `preferences.json` directly, not through a JS round-trip) means any
  migration of `preferences` to redb must update **both** the JS
  (`native.ts`) and Rust (`telemetry.rs`) read paths in the same change —
  a JS-only migration would silently break Rust-native telemetry consent
  checks.

---

```text
RECOMMENDATION:
Use redb — but in a NEW, separate database, not either existing one.

DATABASE:
~/.valori/studio.redb (new file, new StudioDatabase type)

OWNER:
New crate `crates/valori-studio-storage`, depended on only by
`desktop/src-tauri`. Not depended on by valori-daemon, valori-node,
valori-metadata, or valori-consensus.

TABLES:
preferences, recent_projects, project_cache (cache-only), telemetry_queue,
sessions, cloud_project_refs (reference-only, Cloud stays authoritative).
Explicitly NOT: projects/collections (owned by valori-domain +
valori-daemon's project.json), executions (owned by valori-planner +
valori-metadata's MetadataDb), models (owned by valori-models), any
apiKey/secret table (no secrets decision made yet), vectors/WAL/snapshots
(owned by valori-kernel/valori-wire/valori-storage).

MIGRATION ORDER:
1. preferences + recent_projects + project_cache (from preferences.json /
   localStorage, dual-write transition period)
2. telemetry_queue (from events.jsonl)
3. sessions, cloud_project_refs (net-new, no existing data to migrate)
Never: apiKey/credential fields — blocked on a separate secrets decision.

BLOCKERS:
- Secrets decision for apiKey fields (§11) — out of scope for this audit,
  must precede any credential-bearing table.
- Confirm MetadataDb's production wiring status stays nil, or re-audit
  file/process separation if it changes (§1.3, §18).
- No fault-injection/crash test exists yet for redb usage in this codebase
  to lean on as precedent (§13) — worth adding alongside implementation,
  not blocking the audit's conclusion.
```

**STOP — awaiting approval before any implementation.**
