# Valori Studio — S4 Persistence Consolidation Audit

**Status:** Read-only investigation. No source code, dependencies, schemas,
migrations, or runtime behavior were modified as part of this audit.
**Scope:** repository-wide inspection of `desktop/src-tauri/**`,
`ui/src/**`, `crates/**` (principally `valori-studio-storage`,
`valori-metadata`, `valori-daemon`, `valori-models`, `valori-domain`,
`valori-node`, `valori-kernel`/`valori-storage` for the project-data
boundary, and the Cloud `ui/src/app/cloud/**` surface for ownership only).

---

## 1. Executive summary

Studio's persistence architecture is **more consolidated than the prompt's
own "current known architecture" assumes**, with one significant
correction: **`metadata.redb` does not exist on any real user's disk
today.** `valori-metadata::MetadataDb` is a fully implemented, tested
crate, but zero production binary (`valori-node`, `valori-daemon`,
`desktop/src-tauri`) ever calls `MetadataDb::open`. It's wired only as an
`Option<&MetadataDb>` parameter into `valori-planner`'s cache lookup,
which every real caller passes as `None`. Treat `metadata.redb` as
**schema-complete, not deployed** — the same status this audit found for
three of `studio.redb`'s own seven tables (`project_cache`, `sync_state`,
`update_state`).

The real, active desktop persistence surface is smaller and cleaner than
it might appear: `studio.redb` (4 of 7 tables actually written by
production code), the OS keychain (S3, one secret shape), ~14 `localStorage`
keys (S2c/S3 already closed the worst of these — provider secrets — but
non-secret provider config, several caches, and a few small UI-preference
keys remain there, on both desktop and web), and project-owned files under
`~/.valori/projects/<name>/` (untouched by any Studio-metadata concept,
confirmed).

**The highest-priority finding (P0)**: `sessions` remains genuinely
unbounded — confirmed again in this audit, unchanged since the prior
persistence audit flagged it. Every app launch inserts a new row with no
pruning, cap, or retention policy of any kind (unlike `telemetry_queue`,
which has both a 500-row cap and a 7-day prune). A long-lived install
accumulates one row per launch, forever.

**No P0 data-loss or security-conflicting-authority finding was found in
this pass** — S3's credential work already closed the plaintext-secret
gap, and the remaining duplications (see §15) are dormant/schema-only, not
two *active* writers disagreeing about the same fact.

---

## 2. Current persistence architecture (as verified, not as assumed)

```text
~/.valori/
    studio.redb              ACTIVE — 4/7 tables production-wired
    metadata.redb             NOT PRESENT IN PRACTICE — crate exists,
                               never opened by any binary (see §8)
    projects/<name>/
        project.json          ACTIVE — valori-daemon, per-project manifest
        events.log             ACTIVE — audit/event log (standalone)
        snapshot.val            ACTIVE — kernel state snapshot (vectors +
                                 index + graph, embedded, not separate files)
        events-n{id}.log,       ACTIVE — cluster mode, per Raft node
        current-n{id}.snap,
        raft-n{id}.redb,
        node-n{id}.log
    models/<sanitized-id>/    ACTIVE — valori-models, wired into valori-node
    cache/                    NOT FOUND as a Studio concept — see §11
    logs/                     NOT WRITTEN — tracing goes to stdout/stderr
                               only (confirmed unchanged since the DR phase)
    crashes/crash_marker.json ACTIVE — one-shot, desktop only
    downloads/                NOT FOUND as a distinct directory — model
                               downloads land directly in models/<id>/
                               (see §12)
```

`~/.valori/cache/` and `~/.valori/downloads/` (named in the prompt's own
diagram) were **not found** as directories any current code path
constructs or writes to — see §11/§12 for what actually exists instead.
This is stated as `UNKNOWN — no source evidence either way for a future,
not-yet-built cache/download layer`, not as "the prompt is wrong" — it's
possible these are aspirational, forward-looking paths from a design doc
not yet implemented.

---

## 3. Complete persistence matrix

| Data | Current Location | Writer | Reader | Authority | Scope | Lifetime | Classification | Migration Status |
|---|---|---|---|---|---|---|---|---|
| Preferences (theme, language, onboarding, telemetry consent, window state, last page, installation id, workspace/model dir, dock icon, terms) | `studio.redb` `preferences` table | `preferences_service.rs` (all commands), `theme.tsx` (theme), `credential_service.rs`/`lib.rs` (installation_id init) | Same + `AppShellGate.tsx` | AUTHORITATIVE | Desktop | Forever, singleton row | AUTHORITATIVE, DESKTOP_ONLY | Migrated (S2a/S2b-2a) |
| Project registry (name, paths, favorites, recents) | `studio.redb` `projects` table | `project_registry_service.rs` | Same | AUTHORITATIVE (for Studio's *registry* — not project contents) | Desktop | Forever | AUTHORITATIVE, DESKTOP_ONLY | Migrated (S2b-2b) |
| Sessions (app launches) | `studio.redb` `sessions` table | `session_service.rs`, `lib.rs`'s `setup()`/`shutdown_and_exit()` | `session_service.rs` commands | AUTHORITATIVE | Desktop | **Forever, no pruning — confirmed unbounded** | AUTHORITATIVE, DESKTOP_ONLY, **unbounded** | Migrated (S2b-2c) |
| Telemetry queue | `studio.redb` `telemetry_queue` table | `telemetry.rs`'s `enqueue_to_db`/`enqueue_update_event` | `drain_queue` | EPHEMERAL (queue, not a log) | Desktop | Bounded: 500-row cap + 7-day prune | EPHEMERAL, DESKTOP_ONLY | Migrated (S2b-2d) |
| `project_cache` | `studio.redb` `project_cache` table | **none in production** | **none in production** | N/A | — | — | DORMANT (implemented, zero prod call sites) | Not applicable — never activated |
| `sync_state` | `studio.redb` `sync_state` table | **none in production** | **none in production** | N/A | — | — | DORMANT | Not applicable |
| `update_state` | `studio.redb` `update_state` table | **none in production** | **none in production** | N/A | — | — | DORMANT | Not applicable |
| Crash markers | `~/.valori`-adjacent `crashes/crash_marker.json` (Tauri `app_config_dir()`) | `telemetry.rs`'s panic hook | `check_and_clear_crash_marker` | AUTHORITATIVE (one-shot) | Desktop | Deleted on next-launch read | EPHEMERAL, DESKTOP_ONLY | Never migrated into `studio.redb` — deliberate (file write must survive a panicking thread) |
| Logs | **Nowhere** — stdout/stderr only | `tracing_subscriber::fmt()` | Terminal/OS log capture only | N/A | Desktop | Process lifetime | EPHEMERAL | Never implemented |
| Provider config (provider, model, credentialRef) | `localStorage` (`valori:llm_config`/`valori:embedding_config`/`valori:reranker_config`) | the two hooks + 3 reranker call sites (S3) | many components | AUTHORITATIVE | **Desktop AND Web, same keys, different secret-shape (see §14)** | Until cleared | AUTHORITATIVE, WEB_ONLY for the secret shape, shared mechanism otherwise | Secret portion migrated (S3); non-secret portion NOT moved to `studio.redb` (see §9) |
| Provider secret | OS credential store (`keyring`) | `CredentialService` | Same | AUTHORITATIVE | **Desktop only** | Until deleted | AUTHORITATIVE, KEYCHAIN_OWNED, DESKTOP_ONLY | Migrated (S3) |
| Project identity/config (`project.json`) | `~/.valori/projects/<name>/project.json` | `valori-daemon`'s `JsonProjectStore` | `valori-daemon`, `ui/`'s `/api/projects*` routes | AUTHORITATIVE | Desktop (local projects) | Forever | AUTHORITATIVE, PROJECT_OWNED | N/A (daemon-native since RFC-0006) |
| Project vectors/index/graph state | `~/.valori/projects/<name>/snapshot.val` + `events.log` | `valori-node`/`valori-kernel`/`valori-storage` | Same | AUTHORITATIVE | Desktop (local) / Cloud (hosted) | Forever | AUTHORITATIVE, PROJECT_OWNED | N/A |
| `metadata.redb` (projects/collections/planner_cache tables) | Crate exists, **file never created in practice** | `valori-metadata::MetadataDb` (library only) | `valori-planner`'s optional cache param (always `None` from every real caller) | N/A — unwired | — | — | DORMANT — see §8 | Not applicable |
| Model manifests + artifacts | `~/.valori/models/<sanitized-id>/` | `valori-models::ModelStore::install` | `valori-models`, `valori-node` (`ingest.rs`, `server.rs`, `cluster_server.rs`) | AUTHORITATIVE | Desktop + standalone node (same store) | Forever (until GC — `gc.rs` exists) | AUTHORITATIVE, PROJECT-ADJACENT (installation-scoped, not per-project) | N/A |
| Download-in-progress state | In-process `Arc<Mutex<DownloadState>>` (`valori-models::downloader`) | `DownloadJob` | Same process only | EPHEMERAL | Wherever the node process runs | Process lifetime — lost on restart (partial file + SHA re-verify is the resume path) | EPHEMERAL, CACHE-adjacent | N/A |
| `tauri-plugin-store` | Registered (`lib.rs:397`, `Cargo.toml`) but **zero actual store files ever written** — no `@tauri-apps/plugin-store` import anywhere in `ui/src` | None | None | N/A | — | — | LEGACY, vestigial dependency | Superseded by `studio.redb` (S2b-2a); never removed |
| Cloud projects/orgs/keys/tokens/subscriptions | Supabase (Postgres) tables | Cloud server actions (`ui/src/app/cloud/**/actions.ts`) | Same | CLOUD_OWNED | Cloud only | Cloud-managed | AUTHORITATIVE, CLOUD_OWNED | N/A — always Cloud-native |
| Cloud session (Supabase auth) | Cookie (`ui/src/utils/supabase/client.ts`) + `localStorage` mirror (library-managed) | Supabase SSR client | Same | CLOUD_OWNED (source), local cache | Web + Desktop (both reach Cloud via the same client) | Session-scoped | CACHE (local), CLOUD_OWNED (source) | N/A |

---

## 4. localStorage inventory

Every key found, with writer/reader/shape/desktop-vs-web/secret status:

| Key | Writer(s) | Reader(s) | Shape | Desktop | Web | Secret? | Authoritative? | Migration status | Replacement |
|---|---|---|---|---|---|---|---|---|---|
| `valori:llm_config` | `useLLMConfig.ts` | `useLLMConfig.ts` consumers (12+ components) | `{provider, model, endpoint, credentialRef?}` (desktop) / `{..., apiKey}` (web) | Yes | Yes | **No, as of S3** (was yes) | Yes, for non-secret config | Secret portion migrated (S3) | See §9 — non-secret portion intentionally not moved |
| `valori:embedding_config` | `useEmbeddingConfig.ts` | same, plus 2 read-only direct readers (`app/projects/[name]/{page,layout}.tsx`, provider/model/endpoint only, never `apiKey`) | same shape | Yes | Yes | No (S3) | Yes | Secret migrated | Same |
| `valori:reranker_config` | **3 independent writers** — `SettingsModal.tsx`, `app/settings/page.tsx`, and (read-only) `AskTab.tsx` | Same 3 | same shape | Yes | Yes | No (S3) | Yes | Secret migrated (S3, all 3 sites fixed) | Same — flagged in S3's own phase doc as pre-existing UI duplication, not S3's to fix |
| `valori:projects-list` | `useProjectManifest.ts` | Same | `ManifestProject[]` (SWR cache) | Yes | Yes | No | **No — explicitly a cache**, source is `/api/projects` → `valori-daemon` | N/A | CACHE, rebuildable from daemon |
| `valori:archived-projects` | `app/page.tsx` | Same | array of project names | Yes | Yes | No | Yes (no server-side archived-project concept found) | N/A | — |
| `valori:auto-snap:{enabled,threshold,last-count,last-at}` | `app/snapshots/page.tsx` | Same | scalars | Yes | Yes | No | Yes (UI-only preference, no server mirror found) | N/A | — |
| `valori:activity` | `app/page.tsx` | Same | `Record<string, number>` (heatmap) | Yes | Yes | No | Yes | N/A | — |
| `valori:notifs` | `SettingsModal.tsx` | Same | `{desktop?: boolean, ...}` | Yes | Yes | No | Yes | N/A | — |
| `valori:tree:${namespace}` | `AskTab.tsx` | Same | Tree-RAG cache_key/node_count/doc_name | Yes | Yes | No | **No — cache**, rebuildable via `tree_build` | N/A | CACHE |
| `valori-theme` | `theme.tsx` (web branch only, since S2c) | Same | `"light"\|"dark"\|"system"` | **No** (desktop uses `studio.redb.preferences.theme` since S2c) | Yes | No | Yes (web only) | Migrated off desktop (S2c) | `studio.redb` (desktop) |
| `valori:privacy` | **None — dead key**, confirmed by both this audit and prior comments in `native.ts`/`SettingsModal.tsx` (`"the old valori:privacy key here was write-only — nothing ever read it"`) | None | — | — | — | — | LEGACY, unused | Already effectively removed; the string literal is gone from write paths, only mentioned in comments |

**Answering the S3-vs-provider-config question directly (§3 of the task
prompt)**: leaving non-secret provider configuration
(`provider`/`model`/`endpoint`/`credentialRef`) in `localStorage` rather
than moving it into `studio.redb` was **evidence-confirmed intentional**,
not an oversight — S3's own phase doc states this explicitly: *"routing
provider config through `studio.redb` for the first time would have been
an unrelated persistence-location change, out of S3's explicit scope."*
Whether it is **architecturally desirable long-term** is a genuine open
question — see §9 and §14 for the tradeoffs; this audit does not resolve
it, per the read-only mandate.

---

## 5. tauri-plugin-store inventory

- **Dependency**: `desktop/src-tauri/Cargo.toml:27` — `tauri-plugin-store = "2"`.
- **Registration**: `desktop/src-tauri/src/lib.rs:397` —
  `.plugin(tauri_plugin_store::Builder::default().build())`, and
  `"store:default"` is listed in
  `desktop/src-tauri/capabilities/default.json`.
- **Actual usage**: **zero.** No `@tauri-apps/plugin-store` import exists
  anywhere in `ui/src` (confirmed by repository-wide search), and no Rust
  code in `desktop/src-tauri/src` calls into the plugin's store API beyond
  registering the plugin itself. No `.dat`/store file is ever created by
  any current code path.
- **Historical context**: `preferences_service.rs`'s own module doc (S2b-2a
  section) states the S2b-2a phase *"replaced `tauri-plugin-store` and
  `LazyStore("preferences.json")` in `ui/src/lib/native.ts` with typed
  Tauri commands"* — confirming this plugin is a **known-superseded,
  never-removed dependency**, not an active parallel store.
- **Recommendation** (audit-only, not implemented): safe to remove in a
  future cleanup phase — `cargo remove tauri-plugin-store` plus dropping
  the `.plugin(...)` registration and the `store:default` capability
  entry. Low risk (zero call sites to break), low priority (P2 — pure
  dependency hygiene, no functional or security impact).

---

## 6. studio.redb inventory

All 7 tables (schema v1, `crates/valori-studio-storage/src/schema.rs`):

| Table | Schema (value type) | Writer | Reader | Prod call sites | Test-only call sites | Growth | Retention | Authority | Backup/recovery |
|---|---|---|---|---|---|---|---|---|---|
| `meta` | JSON scalars (schema version, migration flags, legacy project names) | `db.rs` internals, `migration.rs` | Same | Yes (every open) | Extensive | Fixed, small (a handful of keys) | N/A | AUTHORITATIVE (schema bookkeeping) | Covered by DR (§16) |
| `preferences` | `StudioPreferences` (singleton) | `preferences_service.rs` | Same + `theme.tsx`, `lib.rs` | Yes | Yes | Fixed (one row) | N/A | AUTHORITATIVE | Covered |
| `projects` | `StudioProjectRecord` | `project_registry_service.rs` | Same | Yes | Yes | Bounded by actual project count | N/A explicit, but naturally small | AUTHORITATIVE (registry only, not project contents) | Covered |
| `project_cache` | `StudioProjectCacheEntry` | **none** | **none** | **0** | 4 (`tests/project_cache.rs`) | N/A | N/A | N/A | Covered (empty table) |
| `sessions` | `StudioSessionRecord` | `session_service.rs`, `lib.rs` | Same | Yes | Yes | **+1 row per launch, unbounded** | **None found** | AUTHORITATIVE | Covered, but growth is the concern, not recoverability |
| `telemetry_queue` | `StudioTelemetryEvent` | `telemetry.rs` | Same | Yes | Yes | Bounded, capped at 500 rows | 7-day prune (`PRUNE_OLDER_THAN_MS`) + delivered/discarded eviction | EPHEMERAL | Covered |
| `sync_state` | `StudioSyncState` | **none** | **none** | **0** | 8 (`tests/sync_and_updates.rs`) | N/A | N/A | N/A | Covered (empty table) |
| `update_state` | `StudioUpdateState` | **none** | **none** | **0** | 8 (`tests/sync_and_updates.rs`) | N/A | N/A | N/A | Covered (empty table) |

**Dormant-table verification**: the task explicitly asked to re-verify
`project_cache`/`sync_state`/`update_state` are still dormant — confirmed
true by direct search (`grep` for each store's type/service name across
`desktop/src-tauri/src` and `ui/src`, zero production hits for all three).
They are implemented (87/99/61 lines respectively) and tested
independently, but never constructed or called outside `#[cfg(test)]`.

**Implemented + actively used**: `meta`, `preferences`, `projects`,
`sessions`, `telemetry_queue`. **Implemented but dormant**: `project_cache`,
`sync_state`, `update_state`.

---

## 7. Project storage boundary

Verified from `crates/valori-daemon/src/project.rs`:

```text
~/.valori/
    projects/
        <name>/
            project.json          — JsonProjectStore, ProjectManifest (id, name,
                                     dim, index kind, cluster config, embedding
                                     config, storage config)
            events.log             — standalone audit/event log (BLAKE3-chained)
            snapshot.val            — kernel state snapshot: vectors, graph
                                     nodes/edges, and index data are ENCODED
                                     TOGETHER inside this one file (V6 format,
                                     per CLAUDE.md's snapshot table) — there is
                                     no separate "indexes/" or "vectors/"
                                     directory on disk; the prompt's suggested
                                     diagram implying separate index/vector
                                     files does not match the actual on-disk
                                     layout
            (cluster mode, replication=3, per node id N):
            events-nN.log, current-nN.snap, raft-nN.redb, node-nN.log
```

`studio.redb` never appears inside a project directory, and no project
directory is ever referenced from inside `studio.redb`'s schema beyond the
`projects` table's `path`/`dir` string field (a plain string, not a live
mount or reference) — confirmed by reading `project_registry_service.rs`
and `crate::project::StudioProjectRecord`. The boundary is real and
respected: **Studio metadata and project data are two genuinely separate
storage systems today**, exactly as the architecture constitution requires.

The task's suggested target diagram (studio.redb → Studio metadata;
projects/ → project.json/WAL/snapshots/indexes/vectors as separate
children) is **directionally correct but factually imprecise** on one
point: indexes and vectors are not separate files, they live inside
`snapshot.val`. This audit corrects that detail rather than silently
adopting it.

---

## 8. metadata.redb boundary

**Finding, stated plainly: `metadata.redb` is not part of the running
system today.**

- `valori-metadata::MetadataDb::open()` doc comment: *"One `MetadataDb` per
  valori installation (`~/.valori/metadata.redb`)"* — describes intended
  design.
- Repository-wide search for `MetadataDb::open`/`MetadataDb::new` outside
  `valori-metadata`'s own source/tests found it referenced in
  `valori-consensus`, `valori-engine`, `valori-domain`, `valori-planner`,
  and `valori-studio-storage` — but in every one of those, the reference is
  either a type import, an error-conversion `impl`, or (for `valori-planner`)
  an **`Option<&MetadataDb>`** parameter on `plan_with_cache()`.
- Search for any real construction/opening of a `MetadataDb` (a call to
  `.open(...)` with a real path, or a `VALORI_METADATA`-style env var) in
  `valori-node/src`, `valori-daemon/src`, or `desktop/src-tauri/src`:
  **zero results.**
- Conclusion: `valori-planner`'s two-layer cache (in-process +
  `MetadataDb`) currently operates as an **in-process-only cache** in
  production, because every real call site passes `db: None`. The durable
  layer is fully coded and tested (`crates/valori-metadata` has its own
  test suite) but not deployed.

**Overlap with `studio.redb`/project storage**: `metadata.redb`'s schema
(`db.rs`) defines `PROJECTS` (keyed by project *name*, JSON `Project`),
`COLLECTIONS`, and `PLANNER_CACHE` tables — a **third** representation of
"project" (see §15), alongside `valori-daemon`'s `project.json`/
`ProjectManifest` and `studio.redb`'s `StudioProjectRecord`. Because
`metadata.redb` is never opened, this is a **dormant** duplication, not an
active conflicting-authority problem — but it is real schema drift worth
resolving before `metadata.redb` is ever actually wired in, so it doesn't
become a fourth active "what is a project" answer.

---

## 9. Cloud boundary

Supabase tables/RPCs found in `ui/src/app/cloud/**` (server actions only —
Cloud's own backend/schema was not inspected, only this repo's client
surface):

| Concept | Owner | Evidence |
|---|---|---|
| Organizations, org membership | CLOUD_OWNED | `org_members`, `org_invitations` tables |
| Cloud projects | CLOUD_OWNED | `.from('projects')` — a **separate** Supabase table, not the same "projects" as `valori-daemon`'s local `project.json` or `studio.redb`'s registry; these are hosted/Cloud projects, a genuinely different concept sharing an English word, not a duplicate authority |
| Collections (Cloud) | **UNKNOWN** — not directly inspected in this pass; Cloud project pages (`cloud/projects/[id]/**`) exist but their exact collection-storage table wasn't traced beyond confirming the route surface exists |
| Deployment/workers | **UNKNOWN** — no `deployment`/`worker` Supabase table reference found in the client surface searched; may live entirely server-side (Cloud control plane, outside this repo per `docs/architecture/control-plane.md`) |
| Cloud API keys, personal access tokens, service accounts | CLOUD_OWNED | `api_keys_public`, `personal_access_tokens_public`, `service_accounts` — hash-stored, reveal-once (confirmed in the S3 credentials audit) |
| Billing | CLOUD_OWNED | `subscriptions` table |
| Provider credentials (OpenAI/etc.) | **Not Cloud-owned at all** — confirmed by S3's audit: no code path uploads a provider credential or its reference to any Supabase table |
| Login history, IP allowlist | CLOUD_OWNED | `login_history`, `ip_allowlist_rules` |
| Cloud session | CLOUD_OWNED (source) / cached locally (cookie + Supabase SSR client's own `localStorage` mirror) | `utils/supabase/client.ts` |

No overlap was found between Cloud-owned data and Studio/desktop-owned
data other than the shared English word "projects" for two genuinely
different things (local vs. hosted). This matches the architecture
constitution's stated separation.

---

## 10. Credential boundary (S3 verification)

- **Where is `credential_ref` currently persisted?** Inside the same
  `localStorage` JSON blob as the rest of that field's provider config
  (`valori:llm_config`/`valori:embedding_config`/`valori:reranker_config`),
  as a `credentialRef` property — not in a dedicated key, not in
  `studio.redb`. Confirmed by reading `useLLMConfig.ts`/
  `useEmbeddingConfig.ts`/the three reranker call sites (all written in
  S3, re-verified in this pass).
- **Should it eventually move into `studio.redb`?** This audit does not
  answer prescriptively (out of scope — audit only), but the evidence
  relevant to a future decision: `credential_ref` is a small, non-secret,
  opaque UUID string — exactly the shape every other `studio.redb`
  preference field already holds (`installation_id` is the closest
  precedent). Moving it would unify "where does Studio keep its own
  metadata" under one system instead of two (`studio.redb` +
  `localStorage`), and would let it survive `studio.redb`'s backup/
  recovery/corruption discipline (§16) — `localStorage` currently has none.
- **Would moving it actually improve authority?** Only if it also
  eliminates the current 3-independent-writer duplication for reranker
  config (§4) — moving the storage location alone, without also unifying
  the write path, would just relocate the duplication, not fix it. This is
  the key tradeoff a future phase would need to resolve, not just the
  storage location.
- **What migration would be required?** A `localStorage` → `studio.redb`
  read-once-then-write migration, following the exact verify-then-clear
  precedent S3 and S2a already established twice in this codebase — low
  novelty, well-trodden pattern here.
- **Should this be a future phase?** Evidence-based recommendation: yes,
  bundled with resolving the reranker-config triplication (§4/§15), not
  as a credential-specific change alone — otherwise it's moving one field
  while leaving the actual architectural problem (three writers) in place.

---

## 11. Telemetry/session/log boundary

| | Owner | Stored | Bounded? | Recoverable? | Uploaded? | Can contain secrets? |
|---|---|---|---|---|---|---|
| Sessions | `session_service.rs` | `studio.redb` `sessions` table | **No — confirmed unbounded, unchanged from the prior audit** | Yes (`studio.redb`'s DR system) | No (sessions themselves are never uploaded — only telemetry events *about* session lifecycle are) | No — `StudioSessionRecord` has no freeform field (id, installation_id, app_version, platform, timestamps, crashed only) |
| Telemetry queue | `telemetry.rs` | `studio.redb` `telemetry_queue` table | Yes — 500-row cap + 7-day prune | Yes (DR system) | Yes, per-event, gated by consent (`drain_queue` → `POST /v1/telemetry/events`) | Structurally possible (`payload`/`properties` is freeform JSON) but no current call site does — confirmed unchanged since S3's audit, and now covered by S3's own regression test |
| Logs | Nowhere (stdout/stderr) | N/A | N/A (not persisted at all) | No | No | N/A |
| Crash markers | `telemetry.rs`'s panic hook | `crash_marker.json`, file-based, outside `studio.redb` | Bounded — one marker, overwritten/deleted each cycle | Not part of `studio.redb`'s DR system (separate file) | Yes, as a telemetry event, gated by crash consent | No — `CrashInfo`'s field list is closed (hash, location, session id, uptime), confirmed by S3's regression test |

**Re-verifying the "sessions is the only genuinely unbounded table" claim
from the prior audit**: confirmed true. `session.rs`'s only bound-adjacent
method is `recent(limit)`, which truncates a **read** result — it does not
delete anything. No prune/retention method exists on `SessionStore` at
all (contrast with `TelemetryQueue::prune_older_than` and its 500-row
cap). This audit does not implement pruning, per the read-only mandate,
but confirms the finding stands and should be P0/P1 priority for S5 (see
§20).

---

## 12. Cache inventory

| Cache | Rebuildable? | Authoritative? | Notes |
|---|---|---|---|
| `valori:projects-list` (`localStorage`) | Yes — refetched from `/api/projects` on every mount via SWR | No | Explicitly documented in its own source comment as "a fallback so the project grid renders instantly," not a source of truth |
| `valori:tree:${namespace}` (`localStorage`) | Yes — rebuildable via `tree_build` | No | Tree-RAG cache key/metadata only, not the tree content itself |
| Model download partial state (in-process) | Yes — SHA-256 re-verified on resume | No | Lost on process restart by design; the partial file on disk plus expected-hash comparison is the actual resume mechanism, not the in-memory `DownloadState` |
| `valori-planner`'s `ExecutionCache` (in-process `RwLock<HashMap>`) | Yes | No | The in-process half of the two-layer cache described in §8 — the durable half (`MetadataDb`) is unwired, so today this cache is **purely in-process and lost on every restart**, which is a real (if minor) missed-caching-opportunity finding, not a correctness risk |
| Cloud responses | **UNKNOWN** — no dedicated client-side response cache found beyond React/SWR's own default in-memory cache (not a persistence mechanism) | — | — |
| "avatars", "schemas" (named in the task prompt) | **UNKNOWN — not found** — no code path matching either concept was located in this repository | — | — |

No cache in this inventory was found acting as an accidental source of
truth — every one identified has a traceable, rebuildable origin.

---

## 13. Update/download/model storage

- **Model metadata + artifacts**: co-located at
  `~/.valori/models/<sanitized-id>/` — `ModelManifest` (metadata) and the
  downloaded artifact live in the same directory, written by
  `ModelStore::install()` (`crates/valori-models/src/lib.rs:202`).
- **Download state**: in-process only (`DownloadJob`/`DownloadState`,
  `Arc<Mutex<_>>`) — not durable. A killed process loses in-memory
  progress; resumption relies on the partial file already on disk plus
  SHA-256 re-verification, not on any persisted "download state" record.
- **Installation state**: `ModelStore::installed()` — derived by scanning
  `models_dir` at call time (a manifest file present = installed), not a
  separate persisted flag.
- **`update_state` (Studio's own table)**: dormant (§6) — Studio's own
  update-checking (`tauri_plugin_updater`) does not currently write to
  `studio.redb`'s `update_state` table at all; that table's schema exists
  for a future integration that hasn't happened yet.
- **Authority**: `valori-models`' on-disk manifest is authoritative for
  "what models are installed" — there is no competing record anywhere
  else (`studio.redb`, `metadata.redb`) claiming the same fact.
- **Marketplace readiness note** (context only, not a recommendation):
  since model storage is already installation-scoped (not per-project) and
  already has its own manifest/GC/integrity-check machinery
  (`valori-models::gc`, `integrity.rs`), it is structurally closer to
  "ready for a marketplace" than `studio.redb`'s dormant `update_state`
  table is to "ready for real update tracking" — an observation, not a
  scope-widening recommendation.

---

## 14. Desktop vs Web matrix

| Data | Desktop | Web | Mechanism |
|---|---:|---:|---|
| Preferences (theme, onboarding, telemetry consent, etc.) | ✅ (authoritative) | ✅ (separate, ephemeral) | `studio.redb` (desktop) / **in-memory JS object only** (web — `native.ts`'s `devMemoryPreferences`, confirmed **not even `localStorage`**, lost on every page refresh) |
| `installationId` | ✅ | ✅ (separate identity) | `studio.redb` (desktop) / `devMemoryPreferences` fallback → effectively regenerated every reload in a real browser tab, since nothing persists it (see finding below) |
| `credentialRef` / provider secret | ✅ | N/A (secret) | OS keychain + `localStorage` (desktop) / `localStorage` `apiKey` directly (web, unchanged since before S3) |
| Provider config (provider/model/endpoint) | ✅ | ✅ | `localStorage`, same keys, both surfaces |
| Project registry | ✅ | N/A | `studio.redb` only — Cloud/web project listing goes through Cloud's own Supabase `projects` table instead, a different concept entirely |
| Sessions | ✅ | ❌ | `studio.redb`, desktop-process-lifecycle concept; no web equivalent found |
| Telemetry | ✅ | **Partially** — `ui/src/lib/telemetry.ts`'s `send()` checks `nativeAvailable()` and no-ops entirely outside Tauri, confirmed by reading `send()`'s first line | `studio.redb` queue (desktop only); web sends nothing via this path |
| Crash reporting | ✅ | ❌ | File-based marker, Tauri-only concept |
| Cloud auth | ✅ | ✅ | Same Supabase SSR client, both surfaces — genuinely shared |
| `sidebar-collapsed`, `valori:notifs`, `valori:activity`, etc. | ✅ | ✅ | Plain `localStorage`, both surfaces identically |

**Notable finding surfaced by building this table**: `getPreference`/
`setPreference`'s web-mode fallback (`devMemoryPreferences`) is a plain JS
object, **not `localStorage`** — meaning `installationId`, `onboardingVersion`,
`recentProjects`, `favoriteProjects`, `lastOpenedProject`, `lastPage`, and
`telemetryConsent` are **not actually persisted at all** in a real browser
tab; they reset on every page load. This is a **pre-existing** (not
introduced by S1-S3) behavior, confirmed by reading `native.ts:48-66`
directly — flagged here because building this matrix is what surfaced it;
no prior audit in this repository's `docs/` explicitly called it out.

---

## 15. Single-source-of-truth matrix

| Concept | Single source of truth? | Where | Two-writer risk? |
|---|---|---|---|
| Preferences | Yes | `studio.redb` (desktop) | No |
| Theme | Yes per-surface | `studio.redb` (desktop) / `localStorage` (web) — S2c already closed the desktop dual-write | No (post-S2c) |
| `InstallationId` | Yes | `studio.redb`, one canonical `get_or_init` (post-Installation-Identity phase) | No |
| `CredentialRef` | Yes (per config key) | `localStorage`, minted by `CredentialService` | No — S3 already fixed the per-keystroke duplicate-minting bug |
| Provider configuration | **No — flagged** | `localStorage`, but reranker config has **3 independent read/write call sites** (`SettingsModal.tsx`, `app/settings/page.tsx`, `AskTab.tsx`) all touching the same key | **Yes — two independent Settings UIs (`SettingsModal.tsx` and `app/settings/page.tsx`) can both write `valori:reranker_config`.** This pre-dates S3 (S3's phase doc flags it as found-not-caused) but is a real, currently-live two-writer situation |
| Project identity | **No single global one** — by design, not by accident: `valori-domain::Project`/`ProjectId` (canonical type), `valori-daemon::ProjectManifest` (persisted, authoritative for local project facts), `valori-metadata::Project` (dormant, unwired), `studio.redb::StudioProjectRecord` (Studio's own registry/cache layer, explicitly *not* authoritative for project contents) | See §7/§8/§15 | Not currently a live conflict — `valori-metadata`'s copy is dormant — but see §15's duplicate-model section for the drift risk if it's ever activated without reconciling |
| Project configuration (embedding/index/storage) | Yes | `project.json`, via `valori-daemon` | No |
| Collections | Yes (per project) | Namespace sidecar file (`.namespaces.json`) read by `ui/`'s `/api/projects` route + the kernel's own namespace registry | No |
| Sessions | Yes | `studio.redb` | No |
| Telemetry | Yes | `studio.redb` queue | No |
| Sync state | N/A — dormant, no writer exists | `studio.redb` (unwritten) | No (nothing to conflict with) |
| Update state | N/A — dormant | `studio.redb` (unwritten) | No |
| Models | Yes | `~/.valori/models/` manifests | No |
| Model metadata | Yes (co-located with the artifact) | Same | No |
| Project cache (`studio.redb`'s table) | N/A — dormant | — | No |
| Cloud projects | Yes | Supabase `projects` table | No (different concept from local projects, not a duplicate) |
| Organizations | Yes | Supabase | No |

**The one flagged, currently-live two-writer situation**: reranker
provider configuration. Everything else checked either has one clear
source of truth or is dormant (no writer at all, so no conflict is
possible yet).

---

## 16. Duplicate-model inventory

| Concept | Representations found | Classification |
|---|---|---|
| **Project** | (1) `valori_domain::{Project, LocalProject, ApiProject}` — canonical cross-boundary type; (2) `valori_daemon::ProjectManifest` — the actual persisted `project.json` shape, richer than the canonical type (cluster config, embedding config, storage config); (3) `valori_metadata::Project` (`db.rs`'s `PROJECTS` table) — dormant, unwired; (4) `valori_studio_storage::StudioProjectRecord` — Studio's own UI-facing registry record; (5) `ui/`'s `ManifestProject`/`ProjectEntry`/`DaemonProject` TypeScript interfaces — DTOs shaping the daemon's JSON for the frontend | (1) canonical type — intentional. (2) authoritative persisted shape — intentional, richer by necessity. (3) **accidental duplication risk if ever activated** — dormant today. (4) intentional DTO/cache layer, explicitly documented as non-authoritative for project contents. (5) intentional TS-side DTOs, adapter-shaped (`toManifestShape` in `project-adapter.ts`) |
| **Provider config** | `LLMConfig`/`EmbeddingConfig` (TS, `ui/src/lib/hooks/`), ad-hoc reranker config objects (3 separate inline shapes in `SettingsModal.tsx`/`settings/page.tsx`/`AskTab.tsx`), `valori_daemon::EmbeddingConfig` (Rust, `project.rs`, `api_key_ref`-shaped) | Reranker: **accidental duplication** (3 near-identical inline TS shapes, no shared type). LLM/Embedding: intentional (one hook each). Daemon's `EmbeddingConfig`: intentional, different purpose (persisted project config, not live UI state), confirmed compatible with `CredentialRef` (S3) |
| **Session** | `valori_domain::SessionId` (canonical id), `StudioSessionRecord` (Rust, persisted), `StudioSessionDto` (Rust→TS wire shape) | Intentional — id/record/DTO is this codebase's consistent pattern (same shape for `ProjectId`/`StudioProjectRecord`) |
| **Credential** | `valori_domain::CredentialRef` (canonical), `valori_daemon::EmbeddingConfig.api_key_ref: Option<String>` (compatible but untyped — confirmed by S3, no adapter needed) | Intentional — `api_key_ref` deliberately stays a plain `String` for manifest backward-compatibility (S3's own documented decision) |
| **Preferences** | `StudioPreferences` (Rust, `studio.redb`), no separate TS interface found (accessed via generic `get_field`/`set_field` string-keyed API, not a typed DTO) | Intentional — the generic key/value bridge is a deliberate simplification, not a duplicate model |
| **Model** | `ModelManifest` (Rust, `valori-models`) — no separate TS interface found beyond whatever `valori-node`'s HTTP responses shape ad hoc | **UNKNOWN** — did not trace every model-related HTTP response shape in `ui/src` in this pass; plausible minor DTO drift, not confirmed |
| **Organization** | Supabase-only (no local/Rust representation found) — out of Studio's persistence scope entirely | Not applicable to this audit |

**Highest-value finding here**: the reranker-config triplication is a real
accidental duplication with a live two-writer risk (§14). The `Project`
type's four/five representations are individually justified except for
`valori-metadata::Project`, which is a latent risk *only if and when*
`metadata.redb` is ever wired into a real deployment — worth resolving
(or deliberately deleting the unused table) before that happens, not
after.

---

## 17. Backup/recovery matrix

| Storage | Backed up? | Recovery? | Machine-bound? | Safe to rebuild? |
|---|---:|---:|---:|---:|
| `studio.redb` | Yes — rolling backups (DR phase) | Yes — `open_with_recovery` (preserve corrupt → try backups → fresh fallback) | No (portable as a file, but see credential/keychain caveat below) | Partially — preferences/sessions/telemetry are lost on total loss, but the app remains launchable (DR guarantee); a fresh install regenerates a working, if empty, `studio.redb` |
| `metadata.redb` | **N/A — file does not exist in practice** (§8) | N/A | N/A | N/A |
| Project files (`project.json`, `events.log`, `snapshot.val`) | **No Studio-level backup found** — these are the user's actual data; backup is presumably the user's own responsibility (filesystem-level) or a future Cloud-sync feature, not something `studio.redb`'s DR system covers | N/A within Studio's DR system | Portable — a project directory can be copied/moved (confirmed by `renaming_a_project_directory_preserves_the_id`-style tests in `valori-daemon`) | No — this is the actual, non-rebuildable user data |
| Models | No dedicated backup found | Re-downloadable (SHA-256 verified against the manifest) | Portable, but large | Yes — fully rebuildable by re-downloading |
| OS keychain (provider secrets) | **Outside Studio's control** — whatever the OS provides (e.g. macOS Keychain's own iCloud Keychain sync, if the user has that enabled) — `studio.redb`'s DR system does not and cannot back this up (confirmed, S3 audit) | OS-native only | **Yes — machine-bound by design** (S3's own documented, intentional property) | No — the secret itself is not rebuildable by Valori; the user must re-enter it |
| Logs | N/A — never persisted | N/A | N/A | N/A |
| Crash markers | No | No (one-shot, self-deleting) | N/A | Yes trivially (losing one isn't consequential) |
| `localStorage` (provider config, caches, preferences) | **No Studio-level backup at all** — ordinary browser/webview storage, no DR discipline applied | No | Yes (webview-profile-bound) | Mixed — caches are rebuildable; provider config (`credentialRef`, provider, model) is not currently backed up anywhere, meaning a corrupted/cleared webview profile loses the *reference* even though the *secret* survives in the keychain — an orphaned-but-recoverable-by-re-linking state, not data loss of the secret itself, but still a real gap versus `studio.redb`'s discipline |
| `~/.valori/downloads/`, `~/.valori/cache/` | N/A — not found as real directories (§2) | N/A | N/A | N/A |

**Restoring `studio.redb` on another machine — consequences, verified**:
`credentialRef`s inside `localStorage` (not `studio.redb` itself, per §10)
would still point at keychain entries that don't exist on the new machine
— re-confirms S3's own documented finding, not new to this audit.
`installation_id` would resolve fine (it's genuinely local, no
cross-machine meaning claimed). `sessions`/`telemetry_queue` would carry
over as historical records — harmless, matches existing DR-phase
documentation.

---

## 18. Failure matrix

| Mechanism | Missing | Corrupt | Locked | Unavailable | Permission denied | Partially written |
|---|---|---|---|---|---|---|
| `studio.redb` | Fresh DB created, app launches normally (confirmed, DR phase) | Recovery pipeline: preserve → try backups → fresh fallback (confirmed, DR phase) | `DatabaseAlreadyOpen` explicitly distinguished from corruption (confirmed, DR phase doc) | Same as corrupt path — DR system treats "can't open" uniformly | Same as corrupt/unavailable — falls through to fresh-database fallback, app still launches (never fails startup) | Handled by redb's own transactional guarantees — a torn write is not possible at the table level; recovery pipeline still applies if the file itself is left in a bad state |
| `project.json` | **UNKNOWN in this pass** — not re-traced in this audit; the S3/prior audits didn't cover this file's own failure modes and this pass didn't either | UNKNOWN | UNKNOWN | UNKNOWN | UNKNOWN | UNKNOWN |
| `events.log`/WAL | **UNKNOWN in this pass** — `valori-storage`'s WAL/recovery logic exists (per CLAUDE.md's crate table) but wasn't re-audited here; out of this audit's practical time budget, flagged rather than guessed at | | | | | |
| `snapshot.val` | Same — UNKNOWN, not re-traced this pass | | | | | |
| OS keychain | `CredentialService::get` returns `Ok(None)` — treated as "not found," not an error (confirmed, S3) | N/A (OS-managed, not Valori's format) | N/A | `CredentialError::Unavailable`, user-safe message, fails closed (confirmed, S3) | `CredentialError::PermissionDenied`, same graceful handling (confirmed, S3) | N/A — `keyring`'s underlying OS APIs are atomic per-entry |
| `localStorage` | Every reader wraps access in `try {} catch {}` — confirmed across every file this and the S3 audit inspected; absence is always treated as "use defaults," never a crash | Same `try/catch` pattern — a JSON parse failure is caught and treated as absent/default in every call site inspected | N/A (not a locking storage system) | N/A (synchronous, always available in a real browser/webview context) | N/A | Not applicable — `localStorage.setItem` is atomic per key at the browser/webview level |

**Explicit UNKNOWNs in this section**: `project.json`, `events.log`, and
`snapshot.val`'s failure-mode handling were not re-traced in this pass —
this audit's scope was Studio persistence specifically, and a full,
rigorous audit of `valori-storage`'s WAL/recovery/crash-safety code (a
large, separate subsystem with its own extensive existing test suite per
CLAUDE.md) was judged out of proportion to this phase's Studio-focused
mandate. Flagged rather than asserted either way.

---

## 19. Migration map

```text
preferences.json          → studio.redb `preferences`         MIGRATED (S2a/S2b-2a)
events.jsonl               → studio.redb `telemetry_queue`      MIGRATED (S2a/S2b-2d)
tauri-plugin-store          → studio.redb (typed commands)       MIGRATED (S2b-2a) — but the
                                                                  OLD dependency was never
                                                                  removed (LEGACY, unused, §5)
localStorage `apiKey`        → OS keychain + `credentialRef`      MIGRATED (S3), desktop only —
  (llm/embedding/reranker)                                       web path deliberately NOT migrated
theme dual-write             → studio.redb only (desktop)         MIGRATED (S2c)
installation_id generation    → unconditional at startup           MIGRATED (Installation Identity phase)
non-secret provider config   → still `localStorage`                NOT MIGRATED — evidence-confirmed
  (provider/model/endpoint)                                       intentional deferral (S3), open
                                                                  question for a future phase (§10)
reranker config triplication  → still 3 independent call sites      NOT MIGRATED — pre-existing,
                                                                  found (not caused) by S3, not yet
                                                                  addressed by any phase
metadata.redb                → never activated                    NOT MIGRATED / NEVER DEPLOYED —
                                                                  schema-complete, zero production
                                                                  wiring (§8)
project_cache/sync_state/    → never activated                    NOT MIGRATED / NEVER DEPLOYED —
  update_state (studio.redb)                                     same status
```

---

## 20. Architectural target

Marking each element **CURRENT** (verified today), **TARGET** (reasonable
future direction the evidence supports), or **UNKNOWN** (no repository
evidence either way):

```text
                    VALORI STUDIO
                         │
             ┌───────────┼───────────┐
             │           │           │
             ▼           ▼           ▼
        studio.redb   Project FS   OS Keychain      [CURRENT — all three exist today]
             │           │           │
       Studio metadata  Real data   Secrets          [CURRENT]
             │           │
             │           └── events.log              [CURRENT — not "WAL" as a
             │           └── snapshot.val                separate file; vectors/
             │               (vectors + index +          index embedded inside
             │                graph embedded)             the snapshot — see §7]
             │
             ├── Preferences                          [CURRENT, ACTIVE]
             ├── Project Registry                      [CURRENT, ACTIVE]
             ├── Sessions                               [CURRENT, ACTIVE, but
             │                                            UNBOUNDED — needs pruning,
             │                                            see §11/§20 P0]
             ├── Telemetry Queue                        [CURRENT, ACTIVE, bounded]
             ├── Sync State                              [CURRENT SCHEMA, TARGET
             │                                            BEHAVIOR NOT YET BUILT]
             ├── Update State                             [CURRENT SCHEMA, TARGET
             │                                            BEHAVIOR NOT YET BUILT]
             └── (TARGET, not current) Provider config    [TARGET — moving
                 reference (credentialRef, provider,        provider config here
                 model) — see §10 for the tradeoff           would unify Studio's
                 this audit surfaces but does not resolve    "own metadata" under
                                                             one system instead of
                                                             two; requires first
                                                             resolving the reranker
                                                             triplication, or it
                                                             just relocates the bug]

metadata.redb                                          [UNKNOWN target status —
                                                          schema-complete but never
                                                          deployed; a real target-
                                                          architecture decision is
                                                          needed: activate it for
                                                          planner caching, or
                                                          formally retire it]
```

---

## 21. Priority findings

### P0
- **`sessions` table is genuinely unbounded** — confirmed, unchanged since
  the prior audit. One row per app launch, forever, no cap, no prune. Not
  a security issue, but a real, currently-accumulating data-growth
  problem for any long-lived install. Ranked P0 because it is *already
  happening* on every real install today, not a hypothetical.
- No security vulnerability or conflicting-authority finding rose to P0 in
  this pass — S3 already closed the plaintext-credential gap, and the
  Project-type duplication (§15/§16) is dormant, not currently conflicting.

### P1
- **Reranker provider configuration has 3 independent read/write call
  sites** (`SettingsModal.tsx`, `app/settings/page.tsx`, `AskTab.tsx`) —
  a live, currently-possible two-writer situation (§14/§15/§16). Not a
  security issue (S3 already ensured none of the three leak the secret),
  but a real architectural-duplication and data-consistency risk (e.g. two
  Settings UIs could disagree about the current reranker provider if a
  user has both open).
- **`metadata.redb` and its `Project`/`Collection`/`PlannerCache` tables
  are schema-complete but never deployed** — not a current risk, but a
  migration/architecture-debt risk: activating it later without first
  reconciling its `Project` representation against `valori-daemon`'s and
  `studio.redb`'s would introduce the exact "two authoritative stores"
  problem this audit was commissioned to prevent.
- **`localStorage` has no backup/recovery discipline at all**, unlike
  `studio.redb` — provider config (`credentialRef`, provider, model) and
  several small preference keys currently live in a storage system with
  no corruption handling, no rolling backups, and no recovery pipeline.
- **`tauri-plugin-store` is a superseded, unused dependency** still
  shipping in the desktop binary — no functional risk, but unnecessary
  attack/maintenance surface.

### P2
- Web-mode preferences (`devMemoryPreferences`) don't actually persist at
  all across a page reload — a real gap, but low severity (web/Cloud is a
  documented lower-durability surface already, per S3's audit) and
  possibly intentional (no evidence either way was found).
- `valori-planner`'s in-process-only cache (since `MetadataDb` is unwired)
  means planner cache benefits reset on every restart — a missed
  optimization, not a correctness issue.
- `~/.valori/cache/` and `~/.valori/downloads/` named in architecture
  diagrams do not exist as real directories — worth reconciling the
  documentation/diagrams with reality, or building them, in a future
  phase.
- `valori:privacy` is a fully dead `localStorage` key reference lingering
  only in comments — trivial cleanup.

---

## 22. Recommended next phases

Based strictly on this audit's evidence (not a commitment, not started):

1. **S5 — Session retention**: add pruning/retention to `studio.redb`'s
   `sessions` table (the one P0 finding). Smallest, most self-contained
   next phase.
2. **S6 — Reranker configuration unification**: collapse the 3 independent
   reranker read/write call sites into one shared implementation (a hook,
   matching `useLLMConfig`/`useEmbeddingConfig`'s existing pattern) —
   resolves the one live two-writer risk found.
3. **S7 — Provider config location decision**: explicitly decide (not
   assume) whether `provider`/`model`/`credentialRef` should move from
   `localStorage` into `studio.redb`, informed by whether S6 has already
   happened (moving before unifying the writers just relocates the bug).
4. **metadata.redb decision**: either formally retire the crate/schema (if
   `valori-planner`'s durable cache layer is no longer planned) or design
   its activation with an explicit reconciliation of the `Project`
   representation against `valori-daemon`'s and `studio.redb`'s — before
   any code wires a real path into it.
5. **Dependency cleanup**: remove `tauri-plugin-store` (P2, trivial, zero
   call sites to break).

## 23. Explicit UNKNOWNs

- `project.json`/`events.log`/`snapshot.val`'s exact failure-mode handling
  (missing/corrupt/locked/permission-denied/partially-written) — not
  re-traced in this pass; `valori-storage`'s WAL/recovery subsystem is
  large and has its own existing test coverage per CLAUDE.md, judged out
  of this Studio-focused audit's proportionate scope.
- Cloud's exact Collection/deployment/worker data ownership — the client-
  side Supabase table references were enumerated, but the Cloud backend
  schema itself (outside this repository, per
  `docs/architecture/control-plane.md`) was not inspected.
- Whether `~/.valori/cache/` and `~/.valori/downloads/` are planned-but-
  unbuilt or simply inaccurate in prior architecture diagrams — no
  evidence found either way.
- Whether any `ModelManifest`-shaped TypeScript DTO exists with drift from
  the Rust type — not exhaustively traced in this pass.
- Whether `valori-daemon`'s `EmbeddingConfig.api_key_ref` is populated by
  any code path today — S3's audit found it is not; this audit did not
  re-verify that specific claim independently, it carries it forward.

---

*End of audit. No source files, dependencies, schemas, migrations, or
runtime behavior were modified. Awaiting approval before any
implementation work begins.*
